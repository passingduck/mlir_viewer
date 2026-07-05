use std::path::Path;

use crate::error::Result;
use crate::identity::{IdentityEvent, IdentityKind, IdentitySource, OpIndexRow, Side};
use crate::writer::{PassId, PassRecord, TraceWriter};

/// Stage snapshots for a miniature torch-to-LLVM pipeline. Index i is the IR
/// *before* child pass i; index i+1 is the IR after it. `cse` is a no-op.
const STAGES: [&str; 6] = [
    // 0: initial import
    r#"module {
  func.func @forward(%arg0: tensor<4x8xf32>, %arg1: tensor<8x4xf32>) -> tensor<4x4xf32> {
    %c = arith.constant dense<0.0> : tensor<4x4xf32>
    %c2 = arith.constant dense<0.0> : tensor<4x4xf32>
    %0 = linalg.matmul ins(%arg0, %arg1 : tensor<4x8xf32>, tensor<8x4xf32>)
        outs(%c : tensor<4x4xf32>) -> tensor<4x4xf32>
    %1 = arith.addf %0, %c2 : tensor<4x4xf32>
    return %1 : tensor<4x4xf32>
  }
}"#,
    // 1: after canonicalize (duplicate constant folded away, addf of zero elided)
    r#"module {
  func.func @forward(%arg0: tensor<4x8xf32>, %arg1: tensor<8x4xf32>) -> tensor<4x4xf32> {
    %c = arith.constant dense<0.0> : tensor<4x4xf32>
    %0 = linalg.matmul ins(%arg0, %arg1 : tensor<4x8xf32>, tensor<8x4xf32>)
        outs(%c : tensor<4x4xf32>) -> tensor<4x4xf32>
    return %0 : tensor<4x4xf32>
  }
}"#,
    // 2: after cse — identical (no-op pass)
    r#"module {
  func.func @forward(%arg0: tensor<4x8xf32>, %arg1: tensor<8x4xf32>) -> tensor<4x4xf32> {
    %c = arith.constant dense<0.0> : tensor<4x4xf32>
    %0 = linalg.matmul ins(%arg0, %arg1 : tensor<4x8xf32>, tensor<8x4xf32>)
        outs(%c : tensor<4x4xf32>) -> tensor<4x4xf32>
    return %0 : tensor<4x4xf32>
  }
}"#,
    // 3: after my-custom-fusion (custom dialect op appears)
    r#"module {
  func.func @forward(%arg0: tensor<4x8xf32>, %arg1: tensor<8x4xf32>) -> tensor<4x4xf32> {
    %0 = mycompiler.fused_matmul %arg0, %arg1 {tile_size = 4 : i64}
        : (tensor<4x8xf32>, tensor<8x4xf32>) -> tensor<4x4xf32>
    return %0 : tensor<4x4xf32>
  }
}"#,
    // 4: after one-shot-bufferize
    r#"module {
  func.func @forward(%arg0: memref<4x8xf32>, %arg1: memref<8x4xf32>) -> memref<4x4xf32> {
    %alloc = memref.alloc() : memref<4x4xf32>
    mycompiler.fused_matmul_buf %arg0, %arg1, %alloc {tile_size = 4 : i64}
        : memref<4x8xf32>, memref<8x4xf32>, memref<4x4xf32>
    return %alloc : memref<4x4xf32>
  }
}"#,
    // 5: after convert-to-llvm
    r#"module {
  llvm.func @forward(%arg0: !llvm.ptr, %arg1: !llvm.ptr) -> !llvm.ptr {
    %0 = llvm.call @mycompiler_fused_matmul(%arg0, %arg1) : (!llvm.ptr, !llvm.ptr) -> !llvm.ptr
    llvm.return %0 : !llvm.ptr
  }
  llvm.func @mycompiler_fused_matmul(!llvm.ptr, !llvm.ptr) -> !llvm.ptr
}"#,
];

const PASS_NAMES: [&str; 5] = [
    "canonicalize",
    "cse",
    "my-custom-fusion",
    "one-shot-bufferize",
    "convert-to-llvm",
];

/// Deterministic demo trace used by CLI/server/UI tests and local development.
pub fn write_demo_trace(path: &Path) -> Result<()> {
    let mut w = TraceWriter::create(path)?;
    w.set_meta("producer", "trace-format fixture 0.1")?;
    w.set_meta("created_at_utc", "2026-07-02T00:00:00Z")?;

    let blobs: Vec<_> = STAGES
        .iter()
        .map(|s| w.write_blob(s))
        .collect::<Result<_>>()?;

    let root = w.record_pass(&PassRecord {
        parent: None,
        seq: 0,
        name: "Pipeline".into(),
        ir_before: Some(blobs[0]),
        ir_after: Some(blobs[5]),
        start_ns: 0,
        end_ns: 5_000_000,
        ir_changed: true,
    })?;

    for (i, name) in PASS_NAMES.iter().enumerate() {
        let before = blobs[i];
        let after = blobs[i + 1];
        w.record_pass(&PassRecord {
            parent: Some(root),
            seq: i as i64,
            name: (*name).into(),
            ir_before: Some(before),
            ir_after: Some(after),
            start_ns: (i as i64) * 1_000_000,
            end_ns: (i as i64 + 1) * 1_000_000,
            ir_changed: before != after,
        })?;
    }
    w.finish()
}

const FULL_STAGES: [&str; 4] = [
    "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.addi %arg0, %arg0 : i32\n  %1 = arith.muli %0, %0 : i32\n  return %1 : i32\n}\n",
    "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 : i32\n  %1 = arith.muli %0, %0 : i32\n  return %1 : i32\n}\n",
    "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 : i32\n  return %0 : i32\n}\n",
    "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 {fast} : i32\n  return %0 : i32\n}\n",
];

fn index_side(
    writer: &mut TraceWriter,
    pass: PassId,
    side: Side,
    text: &str,
    tokens: &[(&str, i64)],
) -> Result<()> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some((_, token)) = tokens.iter().find(|(needle, _)| line.contains(needle)) {
            let byte_start = offset + line.len() - trimmed.len();
            let byte_end = offset + line.trim_end().len();
            let operation = trimmed
                .split_once('=')
                .map(|(_, operation)| operation.trim_start())
                .unwrap_or(trimmed);
            let op_name = operation
                .split(|character: char| character.is_whitespace() || character == '(')
                .next()
                .unwrap_or("unknown")
                .to_string();
            writer.write_op_index(&OpIndexRow {
                pass,
                side,
                ptr_token: *token,
                byte_start: byte_start as i64,
                byte_end: byte_end as i64,
                op_name,
            })?;
        }
        offset += line.len();
    }
    Ok(())
}

/// Deterministic full-fidelity fixture with realistic intra-pass pointer continuity.
pub fn write_full_demo_trace(path: &Path) -> Result<()> {
    let mut writer = TraceWriter::create(path)?;
    writer.set_meta("producer", "trace-format fixture 0.1 (full)")?;
    writer.set_meta("fidelity", "full")?;
    writer.set_meta("created_at_utc", "2026-07-05T00:00:00Z")?;

    let blobs: Vec<_> = FULL_STAGES
        .iter()
        .map(|stage| writer.write_blob(stage))
        .collect::<Result<_>>()?;
    let root = writer.record_pass(&PassRecord {
        parent: None,
        seq: 0,
        name: "Pipeline".into(),
        ir_before: Some(blobs[0]),
        ir_after: Some(blobs[3]),
        start_ns: 0,
        end_ns: 3_000_000,
        ir_changed: true,
    })?;

    struct Step<'a> {
        name: &'a str,
        before_stage: usize,
        after_stage: usize,
        before_tokens: &'a [(&'a str, i64)],
        after_tokens: &'a [(&'a str, i64)],
        event: IdentityEvent,
    }

    let placeholder = PassId(0);
    let steps = [
        Step {
            name: "canonicalize",
            before_stage: 0,
            after_stage: 1,
            before_tokens: &[
                ("func.func", 0x1000),
                ("arith.addi", 0x1001),
                ("arith.muli", 0x1002),
                ("return", 0x1003),
            ],
            after_tokens: &[
                ("func.func", 0x1000),
                ("arith.shli", 0x1081),
                ("arith.muli", 0x1002),
                ("return", 0x1003),
            ],
            event: IdentityEvent {
                pass: placeholder,
                kind: IdentityKind::Replaced,
                ptr_token: 0x1001,
                new_token: Some(0x1081),
                pattern: Some("AddIToShift".into()),
                source: IdentitySource::Listener,
                seq: 0,
            },
        },
        Step {
            name: "dce",
            before_stage: 1,
            after_stage: 2,
            before_tokens: &[
                ("func.func", 0x2000),
                ("arith.shli", 0x2001),
                ("arith.muli", 0x2002),
                ("return", 0x2003),
            ],
            after_tokens: &[
                ("func.func", 0x2000),
                ("arith.shli", 0x2001),
                ("return", 0x2003),
            ],
            event: IdentityEvent {
                pass: placeholder,
                kind: IdentityKind::Erased,
                ptr_token: 0x2002,
                new_token: None,
                pattern: None,
                source: IdentitySource::Listener,
                seq: 0,
            },
        },
        Step {
            name: "set-attr",
            before_stage: 2,
            after_stage: 3,
            before_tokens: &[
                ("func.func", 0x3000),
                ("arith.shli", 0x3001),
                ("return", 0x3003),
            ],
            after_tokens: &[
                ("func.func", 0x3000),
                ("arith.shli", 0x3001),
                ("return", 0x3003),
            ],
            event: IdentityEvent {
                pass: placeholder,
                kind: IdentityKind::Modified,
                ptr_token: 0x3001,
                new_token: None,
                pattern: Some("SetFastAttr".into()),
                source: IdentitySource::Listener,
                seq: 0,
            },
        },
    ];

    for (sequence, step) in steps.into_iter().enumerate() {
        let pass = writer.record_pass(&PassRecord {
            parent: Some(root),
            seq: sequence as i64,
            name: step.name.into(),
            ir_before: Some(blobs[step.before_stage]),
            ir_after: Some(blobs[step.after_stage]),
            start_ns: sequence as i64 * 1_000_000,
            end_ns: (sequence as i64 + 1) * 1_000_000,
            ir_changed: true,
        })?;
        index_side(
            &mut writer,
            pass,
            Side::Before,
            FULL_STAGES[step.before_stage],
            step.before_tokens,
        )?;
        index_side(
            &mut writer,
            pass,
            Side::After,
            FULL_STAGES[step.after_stage],
            step.after_tokens,
        )?;
        writer.write_identity_event(&IdentityEvent { pass, ..step.event })?;
    }

    writer.finish()
}
