use std::path::Path;

use crate::error::Result;
use crate::writer::{PassRecord, TraceWriter};

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
