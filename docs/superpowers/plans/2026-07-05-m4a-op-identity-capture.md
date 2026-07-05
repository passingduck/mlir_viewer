# M4a — Op Identity Capture & Schema Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retire the project's #1 technical risk (parent spec §9) by capturing real MLIR op-lifecycle events into trace schema v2, and ship a pure-Rust synthetic identity fixture so M4b can be built without the MLIR toolchain.

**Architecture:** Two halves. **Rust half** (deterministic, no MLIR): trace schema v2 adds `op_index` and `op_identity` tables, a version-tolerant reader (opens v1 and v2; v1 identity reads empty), writer methods, and a `gen-fixture --full` demo trace. **C++ half** (needs MLIR 21.1): `Fidelity::Full` capture installs a `RewriterBase::Listener` + `Actions` handler and records op byte-index + lifecycle events; delivered as a time-boxed research spike with a findings doc. C++ never assigns durable uids — events are keyed by an intra-pass `Operation*` token; all cross-pass stitching is M4b.

**Tech Stack:** Rust (`rusqlite`, `zstd`, `xxhash-rust`, `clap`), C++17 (MLIR/LLVM 21.1, SQLite3, zstd, xxHash), CMake + Ninja + CTest.

## Global Constraints

- **Trace format version bumps `"1" → "2"`.** `FORMAT_VERSION` in both `crates/trace-format/src/schema.rs` and `capture/lib/TraceStorage.cpp` must read `"2"`.
- **Reader is version-tolerant:** accepts `"1"` and `"2"`; a v1 trace (no identity tables) opens successfully and identity accessors return empty. Never widen to accept unknown versions.
- **C++ and Rust SQL schemas must stay identical.** The `op_index` / `op_identity` DDL is duplicated in `schema.rs` (`SCHEMA_SQL`) and `TraceStorage.cpp` (`schemaSql`); any column change touches both.
- **C++ assigns no durable uids.** Events reference ops only by an intra-pass `Operation*` token (`ptr_token`); pointers are meaningful only within one pass's event stream + that pass's `op_index`.
- **Identity capture is additive and optional.** Only `Fidelity::Full` writes identity tables; `Timeline`/`Text` behavior is unchanged and existing C++ recorder tests must still pass.
- **Confidence interpretation is deferred to M4b.** M4a stores the raw event `source` (`listener` | `action`); it does not compute solid-vs-dashed confidence.
- **TDD throughout.** Every code task writes a failing test first, shows it fail, then implements. Test output must be pristine.
- **Cargo toolchain:** cargo is not on PATH in the execution environment. Prepend `/Users/sungjin/.rustup/toolchains/stable-aarch64-apple-darwin/bin` to PATH for all cargo commands.
- **MLIR toolchain (C++ tasks only):** configure against the stable release: `MLIR_DIR=/Users/sungjin/work/mlir-release/lib/cmake/mlir`, `LLVM_DIR=/Users/sungjin/work/mlir-release/lib/cmake/llvm`.

## SQL Schema Added (verbatim, both languages)

```sql
CREATE TABLE op_index (
    id         INTEGER PRIMARY KEY,
    pass_id    INTEGER NOT NULL REFERENCES pass_execution(id),
    side       INTEGER NOT NULL,       -- 0 = before, 1 = after
    ptr_token  INTEGER NOT NULL,       -- intra-pass Operation* identity (opaque)
    byte_start INTEGER NOT NULL,
    byte_end   INTEGER NOT NULL,
    op_name    TEXT NOT NULL
);
CREATE INDEX idx_op_index_pass ON op_index(pass_id, side);

CREATE TABLE op_identity (
    id         INTEGER PRIMARY KEY,
    pass_id    INTEGER NOT NULL REFERENCES pass_execution(id),
    kind       TEXT NOT NULL,          -- inserted | erased | replaced | modified
    ptr_token  INTEGER NOT NULL,
    new_token  INTEGER,                -- replacement token, for 'replaced'
    pattern    TEXT,                    -- action/pattern name if attributed
    source     TEXT NOT NULL,          -- listener | action
    seq        INTEGER NOT NULL
);
CREATE INDEX idx_op_identity_pass ON op_identity(pass_id, seq);
```

**String encodings (canonical, both languages):** `side` ∈ {0,1}; `kind` ∈ {`inserted`,`erased`,`replaced`,`modified`}; `source` ∈ {`listener`,`action`}.

---

## Task 1: Rust schema v2 + version-tolerant reader

Bump the format version, add the identity DDL to the Rust schema, and make the reader accept both v1 and v2. No writer/reader accessors yet — this task only proves the version gate and that a fresh trace carries the new (empty) tables.

**Files:**
- Modify: `crates/trace-format/src/schema.rs`
- Modify: `crates/trace-format/src/reader.rs:41-61` (the `open` version check)
- Test: `crates/trace-format/tests/identity.rs` (create)

**Interfaces:**
- Consumes: `TraceWriter::create`, `TraceReader::open`, `schema::FORMAT_VERSION`.
- Produces: `schema::FORMAT_VERSION == "2"`; `schema::SUPPORTED_VERSIONS: &[&str]`; a v2 trace has empty `op_index`/`op_identity` tables; `TraceReader::open` succeeds on both `"1"` and `"2"` and still rejects other values with `TraceError::VersionMismatch`.

- [ ] **Step 1: Write the failing test**

Create `crates/trace-format/tests/identity.rs`:

```rust
use rusqlite::Connection;
use tempfile::tempdir;
use trace_format::{TraceReader, TraceWriter};

/// A freshly created trace declares version 2 and carries the (empty)
/// identity tables.
#[test]
fn fresh_trace_is_v2_with_empty_identity_tables() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("v2.mlirtrace");
    let w = TraceWriter::create(&path).unwrap();
    w.finish().unwrap();

    let reader = TraceReader::open(&path).unwrap();
    assert_eq!(reader.meta().unwrap().get("format_version").unwrap(), "2");

    // Tables exist and are empty.
    let conn = Connection::open(&path).unwrap();
    for table in ["op_index", "op_identity"] {
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "table {table} should exist");
    }
}

/// A hand-built v1 trace (no identity tables) still opens: the reader is
/// version-tolerant.
#[test]
fn v1_trace_still_opens() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("v1.mlirtrace");
    // Minimal v1 file: meta table + format_version = "1", nothing else.
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
         INSERT INTO meta(key, value) VALUES ('format_version', '1');",
    )
    .unwrap();
    drop(conn);

    let reader = TraceReader::open(&path).expect("v1 trace must open");
    assert_eq!(reader.meta().unwrap().get("format_version").unwrap(), "1");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p trace-format --test identity`
Expected: FAIL — `fresh_trace_is_v2_with_empty_identity_tables` sees `format_version == "1"` and missing tables; the crate may not yet expose what's needed.

- [ ] **Step 3: Edit `schema.rs`**

Set the version and add the DDL. Replace the top of `crates/trace-format/src/schema.rs`:

```rust
/// Trace format major version written by this build.
pub const FORMAT_VERSION: &str = "2";

/// Versions this build can read. v1 traces predate identity capture and lack
/// the `op_index` / `op_identity` tables, which read as empty.
pub const SUPPORTED_VERSIONS: &[&str] = &["1", "2"];
```

Append the two tables to the `SCHEMA_SQL` string, immediately before the closing `"#;` (after the `idx_pass_parent` index):

```sql
CREATE TABLE op_index (
    id         INTEGER PRIMARY KEY,
    pass_id    INTEGER NOT NULL REFERENCES pass_execution(id),
    side       INTEGER NOT NULL,
    ptr_token  INTEGER NOT NULL,
    byte_start INTEGER NOT NULL,
    byte_end   INTEGER NOT NULL,
    op_name    TEXT NOT NULL
);
CREATE INDEX idx_op_index_pass ON op_index(pass_id, side);

CREATE TABLE op_identity (
    id         INTEGER PRIMARY KEY,
    pass_id    INTEGER NOT NULL REFERENCES pass_execution(id),
    kind       TEXT NOT NULL,
    ptr_token  INTEGER NOT NULL,
    new_token  INTEGER,
    pattern    TEXT,
    source     TEXT NOT NULL,
    seq        INTEGER NOT NULL
);
CREATE INDEX idx_op_identity_pass ON op_identity(pass_id, seq);
```

- [ ] **Step 4: Edit `reader.rs` version check**

In `crates/trace-format/src/reader.rs`, change the import and the `match` in `open`:

Replace `use crate::schema::FORMAT_VERSION;` with:

```rust
use crate::schema::{FORMAT_VERSION, SUPPORTED_VERSIONS};
```

Replace the `match version { … }` block (currently rejecting `v != FORMAT_VERSION`) with:

```rust
match version {
    None => return Err(TraceError::Corrupt("missing format_version".into())),
    Some(v) if !SUPPORTED_VERSIONS.contains(&v.as_str()) => {
        return Err(TraceError::VersionMismatch {
            found: v,
            supported: FORMAT_VERSION,
        })
    }
    Some(_) => {}
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p trace-format --test identity`
Expected: PASS (2 tests). Then `cargo test -p trace-format` — all existing tests still pass.

- [ ] **Step 6: Commit**

```bash
git add crates/trace-format/src/schema.rs crates/trace-format/src/reader.rs crates/trace-format/tests/identity.rs
git commit -m "feat(trace-format): schema v2 identity tables + version-tolerant reader"
```

---

## Task 2: Identity value types + writer/reader accessors

Add the shared identity value types and the write/read paths for `op_index` and `op_identity`, with empty-safe reads on v1 traces (missing tables).

**Files:**
- Create: `crates/trace-format/src/identity.rs`
- Modify: `crates/trace-format/src/lib.rs` (add `mod identity;` + re-exports)
- Modify: `crates/trace-format/src/writer.rs`
- Modify: `crates/trace-format/src/reader.rs`
- Test: `crates/trace-format/tests/identity.rs` (append)

**Interfaces:**
- Consumes: `PassId`, `TraceWriter`, `TraceReader`, `Result`.
- Produces (all `pub`, re-exported from crate root):
  - `enum Side { Before, After }` with `fn to_i64(self) -> i64`, `fn from_i64(i64) -> Option<Side>`
  - `enum IdentityKind { Inserted, Erased, Replaced, Modified }` with `fn as_str(self) -> &'static str`, `fn from_str(&str) -> Option<Self>`
  - `enum IdentitySource { Listener, Action }` with `fn as_str(self) -> &'static str`, `fn from_str(&str) -> Option<Self>`
  - `struct OpIndexRow { pass: PassId, side: Side, ptr_token: i64, byte_start: i64, byte_end: i64, op_name: String }`
  - `struct IdentityEvent { pass: PassId, kind: IdentityKind, ptr_token: i64, new_token: Option<i64>, pattern: Option<String>, source: IdentitySource, seq: i64 }`
  - `TraceWriter::write_op_index(&mut self, row: &OpIndexRow) -> Result<()>`
  - `TraceWriter::write_identity_event(&mut self, ev: &IdentityEvent) -> Result<()>`
  - `TraceReader::op_index(&self, pass: PassId) -> Result<Vec<OpIndexRow>>` (empty if table absent)
  - `TraceReader::identity_events(&self, pass: PassId) -> Result<Vec<IdentityEvent>>` (empty if table absent, ordered by `seq`)

- [ ] **Step 1: Write the failing test (append to `tests/identity.rs`)**

```rust
use trace_format::{
    IdentityEvent, IdentityKind, IdentitySource, OpIndexRow, PassId, PassRecord, Side,
};

#[test]
fn op_index_and_identity_round_trip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("rt.mlirtrace");
    let mut w = TraceWriter::create(&path).unwrap();
    let before = w.write_blob("%0 = arith.constant 1 : i32\n").unwrap();
    let after = w.write_blob("return\n").unwrap();
    let pass = w
        .record_pass(&PassRecord {
            parent: None,
            seq: 0,
            name: "dce".into(),
            ir_before: Some(before),
            ir_after: Some(after),
            start_ns: 0,
            end_ns: 1,
            ir_changed: true,
        })
        .unwrap();

    w.write_op_index(&OpIndexRow {
        pass,
        side: Side::Before,
        ptr_token: 4096,
        byte_start: 0,
        byte_end: 27,
        op_name: "arith.constant".into(),
    })
    .unwrap();
    w.write_identity_event(&IdentityEvent {
        pass,
        kind: IdentityKind::Erased,
        ptr_token: 4096,
        new_token: None,
        pattern: Some("DeadCodeElimination".into()),
        source: IdentitySource::Listener,
        seq: 0,
    })
    .unwrap();
    w.finish().unwrap();

    let reader = TraceReader::open(&path).unwrap();
    let idx = reader.op_index(pass).unwrap();
    assert_eq!(idx.len(), 1);
    assert_eq!(idx[0].op_name, "arith.constant");
    assert_eq!(idx[0].side, Side::Before);
    assert_eq!(idx[0].byte_end, 27);

    let events = reader.identity_events(pass).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, IdentityKind::Erased);
    assert_eq!(events[0].new_token, None);
    assert_eq!(events[0].pattern.as_deref(), Some("DeadCodeElimination"));
    assert_eq!(events[0].source, IdentitySource::Listener);
}

/// On a v1 trace the identity accessors return empty rather than erroring.
#[test]
fn identity_accessors_empty_on_v1() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("v1b.mlirtrace");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
         INSERT INTO meta(key, value) VALUES ('format_version', '1');",
    )
    .unwrap();
    drop(conn);

    let reader = TraceReader::open(&path).unwrap();
    assert!(reader.op_index(PassId(1)).unwrap().is_empty());
    assert!(reader.identity_events(PassId(1)).unwrap().is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p trace-format --test identity`
Expected: FAIL — `write_op_index`, `OpIndexRow`, etc. do not exist (compile error).

- [ ] **Step 3: Create `crates/trace-format/src/identity.rs`**

```rust
//! Shared value types for op identity capture (schema v2) and their canonical
//! SQL string encodings. Used by both the writer and the reader.

use crate::writer::PassId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
}

impl Side {
    pub fn to_i64(self) -> i64 {
        match self {
            Side::Before => 0,
            Side::After => 1,
        }
    }

    pub fn from_i64(v: i64) -> Option<Side> {
        match v {
            0 => Some(Side::Before),
            1 => Some(Side::After),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityKind {
    Inserted,
    Erased,
    Replaced,
    Modified,
}

impl IdentityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IdentityKind::Inserted => "inserted",
            IdentityKind::Erased => "erased",
            IdentityKind::Replaced => "replaced",
            IdentityKind::Modified => "modified",
        }
    }

    pub fn from_str(s: &str) -> Option<IdentityKind> {
        match s {
            "inserted" => Some(IdentityKind::Inserted),
            "erased" => Some(IdentityKind::Erased),
            "replaced" => Some(IdentityKind::Replaced),
            "modified" => Some(IdentityKind::Modified),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySource {
    Listener,
    Action,
}

impl IdentitySource {
    pub fn as_str(self) -> &'static str {
        match self {
            IdentitySource::Listener => "listener",
            IdentitySource::Action => "action",
        }
    }

    pub fn from_str(s: &str) -> Option<IdentitySource> {
        match s {
            "listener" => Some(IdentitySource::Listener),
            "action" => Some(IdentitySource::Action),
            _ => None,
        }
    }
}

/// An op's byte span within one side's IR snapshot text, keyed by an intra-pass
/// `Operation*` token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpIndexRow {
    pub pass: PassId,
    pub side: Side,
    pub ptr_token: i64,
    pub byte_start: i64,
    pub byte_end: i64,
    pub op_name: String,
}

/// One op-lifecycle event observed during a pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityEvent {
    pub pass: PassId,
    pub kind: IdentityKind,
    pub ptr_token: i64,
    pub new_token: Option<i64>,
    pub pattern: Option<String>,
    pub source: IdentitySource,
    pub seq: i64,
}
```

- [ ] **Step 4: Wire re-exports in `lib.rs`**

Add `mod identity;` alongside the other module declarations and re-export the types. Add:

```rust
mod identity;
pub use identity::{IdentityEvent, IdentityKind, IdentitySource, OpIndexRow, Side};
```

(Place the `pub use` next to the existing `pub use writer::{…}` / `pub use reader::{…}` lines.)

- [ ] **Step 5: Add writer methods in `writer.rs`**

Add `use crate::identity::{IdentityEvent, OpIndexRow};` to the imports, then add these methods inside `impl TraceWriter` (before `finish`):

```rust
pub fn write_op_index(&mut self, row: &OpIndexRow) -> Result<()> {
    self.conn.execute(
        "INSERT INTO op_index
         (pass_id, side, ptr_token, byte_start, byte_end, op_name)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            row.pass.0,
            row.side.to_i64(),
            row.ptr_token,
            row.byte_start,
            row.byte_end,
            row.op_name,
        ],
    )?;
    Ok(())
}

pub fn write_identity_event(&mut self, ev: &IdentityEvent) -> Result<()> {
    self.conn.execute(
        "INSERT INTO op_identity
         (pass_id, kind, ptr_token, new_token, pattern, source, seq)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            ev.pass.0,
            ev.kind.as_str(),
            ev.ptr_token,
            ev.new_token,
            ev.pattern,
            ev.source.as_str(),
            ev.seq,
        ],
    )?;
    Ok(())
}
```

- [ ] **Step 6: Add reader accessors in `reader.rs`**

Add `use crate::identity::{IdentityEvent, IdentityKind, IdentitySource, OpIndexRow, Side};` to imports, then add these methods inside `impl TraceReader`. Include a private `has_table` helper for v1 tolerance:

```rust
fn has_table(&self, name: &str) -> Result<bool> {
    let n: i64 = self.conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
        params![name],
        |r| r.get(0),
    )?;
    Ok(n == 1)
}

pub fn op_index(&self, pass: PassId) -> Result<Vec<OpIndexRow>> {
    if !self.has_table("op_index")? {
        return Ok(Vec::new());
    }
    let mut stmt = self.conn.prepare(
        "SELECT side, ptr_token, byte_start, byte_end, op_name
         FROM op_index WHERE pass_id = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![pass.0], |r| {
        let side_raw: i64 = r.get(0)?;
        Ok(OpIndexRow {
            pass,
            side: Side::from_i64(side_raw).unwrap_or(Side::Before),
            ptr_token: r.get(1)?,
            byte_start: r.get(2)?,
            byte_end: r.get(3)?,
            op_name: r.get(4)?,
        })
    })?;
    rows.collect::<std::result::Result<_, _>>().map_err(Into::into)
}

pub fn identity_events(&self, pass: PassId) -> Result<Vec<IdentityEvent>> {
    if !self.has_table("op_identity")? {
        return Ok(Vec::new());
    }
    let mut stmt = self.conn.prepare(
        "SELECT kind, ptr_token, new_token, pattern, source, seq
         FROM op_identity WHERE pass_id = ?1 ORDER BY seq",
    )?;
    let rows = stmt.query_map(params![pass.0], |r| {
        let kind_raw: String = r.get(0)?;
        let source_raw: String = r.get(4)?;
        Ok(IdentityEvent {
            pass,
            kind: IdentityKind::from_str(&kind_raw).unwrap_or(IdentityKind::Modified),
            ptr_token: r.get(1)?,
            new_token: r.get(2)?,
            pattern: r.get(3)?,
            source: IdentitySource::from_str(&source_raw).unwrap_or(IdentitySource::Listener),
            seq: r.get(5)?,
        })
    })?;
    rows.collect::<std::result::Result<_, _>>().map_err(Into::into)
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p trace-format --test identity`
Expected: PASS (4 tests total). Then `cargo test -p trace-format && cargo clippy -p trace-format` — clean.

- [ ] **Step 8: Commit**

```bash
git add crates/trace-format/src/identity.rs crates/trace-format/src/lib.rs crates/trace-format/src/writer.rs crates/trace-format/src/reader.rs crates/trace-format/tests/identity.rs
git commit -m "feat(trace-format): op_index/op_identity writer + reader accessors"
```

---

## Task 3: Synthetic `--full` fixture + CLI flag

Add a pure-Rust demo trace that carries a realistic identity event stream, and expose it via `mlir-viewer dev gen-fixture --full`. This is the decoupling seam: M4b's engine and UI test against this without the MLIR toolchain.

**Files:**
- Modify: `crates/trace-format/src/fixture.rs`
- Modify: `crates/cli/src/main.rs`
- Test: `crates/trace-format/tests/identity.rs` (append)

**Interfaces:**
- Consumes: `TraceWriter`, `PassRecord`, `OpIndexRow`, `IdentityEvent`, `IdentityKind`, `IdentitySource`, `Side`.
- Produces: `fixture::write_full_demo_trace(path: &Path) -> Result<()>` — writes a 3-pass pipeline (`canonicalize` replaces an op, `dce` erases an op, `set-attr` modifies in place) with matching `op_index` rows on both sides; CLI `gen-fixture --full` invokes it.

- [ ] **Step 1: Write the failing test (append to `tests/identity.rs`)**

```rust
use trace_format::fixture::write_full_demo_trace;

#[test]
fn full_fixture_has_scripted_identity_stream() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("full.mlirtrace");
    write_full_demo_trace(&path).unwrap();

    let reader = TraceReader::open(&path).unwrap();
    assert_eq!(reader.meta().unwrap().get("fidelity").unwrap(), "full");

    // Collect the leaf passes (children of the root "Pipeline").
    let roots = reader.passes().unwrap();
    let leaves = &roots[0].children;
    let names: Vec<_> = leaves.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["canonicalize", "dce", "set-attr"]);

    // Each scripted pass carries exactly the expected event kind.
    let kind_of = |pass_name: &str| -> IdentityKind {
        let p = leaves.iter().find(|p| p.name == pass_name).unwrap();
        let evs = reader.identity_events(p.id).unwrap();
        assert!(!evs.is_empty(), "{pass_name} should have events");
        evs[0].kind
    };
    assert_eq!(kind_of("canonicalize"), IdentityKind::Replaced);
    assert_eq!(kind_of("dce"), IdentityKind::Erased);
    assert_eq!(kind_of("set-attr"), IdentityKind::Modified);

    // op_index rows point into the actual blob text on both sides.
    let canon = leaves.iter().find(|p| p.name == "canonicalize").unwrap();
    let after_text = reader.blob_text(canon.ir_after.unwrap()).unwrap();
    let idx = reader.op_index(canon.id).unwrap();
    let after_rows: Vec<_> = idx.iter().filter(|r| r.side == Side::After).collect();
    assert!(!after_rows.is_empty());
    for r in after_rows {
        let start = r.byte_start as usize;
        let end = r.byte_end as usize;
        assert!(end <= after_text.len() && start <= end);
        // op_name is the leading token of its recorded span.
        assert!(after_text[start..end].contains(r.op_name.split('.').last().unwrap()));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p trace-format --test identity`
Expected: FAIL — `write_full_demo_trace` does not exist.

- [ ] **Step 3: Add `write_full_demo_trace` to `fixture.rs`**

Append to `crates/trace-format/src/fixture.rs`. This authors three small snapshots and, for each pass, records `op_index` rows by locating each op's line in the blob text (deterministic `find`), plus one lifecycle event per pass:

```rust
use crate::identity::{IdentityEvent, IdentityKind, IdentitySource, OpIndexRow, Side};
use crate::writer::PassId;

/// Three IR snapshots for the identity demo. Each op sits on its own line so
/// `op_index` byte spans are the line spans.
const FULL_STAGES: [&str; 4] = [
    // 0: before canonicalize
    "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.addi %arg0, %arg0 : i32\n  %1 = arith.muli %0, %0 : i32\n  return %1 : i32\n}\n",
    // 1: after canonicalize (addi replaced by a shift)
    "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 : i32\n  %1 = arith.muli %0, %0 : i32\n  return %1 : i32\n}\n",
    // 2: after dce (muli erased, return uses %0)
    "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 : i32\n  return %0 : i32\n}\n",
    // 3: after set-attr (shli gains an attribute, modified in place)
    "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 {fast} : i32\n  return %0 : i32\n}\n",
];

/// Record one `op_index` row per non-brace line of `text`, on `side`, assigning
/// ptr tokens from `base` upward. Returns the token assigned to the op whose
/// text contains `needle` (for wiring events), or `base` if none matched.
fn index_side(
    w: &mut TraceWriter,
    pass: PassId,
    side: Side,
    text: &str,
    base: i64,
    needle: &str,
) -> Result<i64> {
    let mut token = base;
    let mut found = base;
    let mut offset = 0usize;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let is_op = trimmed.contains('.') && !trimmed.starts_with('}');
        if is_op || trimmed.starts_with("return") || trimmed.starts_with("func.func") {
            let start = offset + (line.len() - trimmed.len());
            let end = offset + line.trim_end().len();
            let op_name = trimmed
                .split(['=', ' '])
                .find(|t| t.contains('.') || *t == "return")
                .unwrap_or("unknown")
                .trim()
                .to_string();
            w.write_op_index(&OpIndexRow {
                pass,
                side,
                ptr_token: token,
                byte_start: start as i64,
                byte_end: end as i64,
                op_name,
            })?;
            if line.contains(needle) {
                found = token;
            }
            token += 1;
        }
        offset += line.len();
    }
    Ok(found)
}

/// Deterministic demo trace at `Fidelity::Full` — text snapshots plus a scripted
/// identity event stream. Consumed by M4b engine/UI tests (no MLIR needed).
pub fn write_full_demo_trace(path: &Path) -> Result<()> {
    let mut w = TraceWriter::create(path)?;
    w.set_meta("producer", "trace-format fixture 0.1 (full)")?;
    w.set_meta("fidelity", "full")?;
    w.set_meta("created_at_utc", "2026-07-05T00:00:00Z")?;

    let blobs: Vec<_> = FULL_STAGES
        .iter()
        .map(|s| w.write_blob(s))
        .collect::<Result<_>>()?;

    let root = w.record_pass(&PassRecord {
        parent: None,
        seq: 0,
        name: "Pipeline".into(),
        ir_before: Some(blobs[0]),
        ir_after: Some(blobs[3]),
        start_ns: 0,
        end_ns: 3_000_000,
        ir_changed: true,
    })?;

    // (pass name, before stage, after stage, event kind, pattern, needle for the subject op)
    let steps = [
        ("canonicalize", 0usize, 1usize, IdentityKind::Replaced, Some("AddIToShift"), "arith.addi", "arith.shli"),
        ("dce", 1, 2, IdentityKind::Erased, None, "arith.muli", "return"),
        ("set-attr", 2, 3, IdentityKind::Modified, Some("SetFastAttr"), "arith.shli", "arith.shli"),
    ];

    for (i, (name, before_s, after_s, kind, pattern, before_needle, after_needle)) in
        steps.into_iter().enumerate()
    {
        let before = blobs[before_s];
        let after = blobs[after_s];
        let pass = w.record_pass(&PassRecord {
            parent: Some(root),
            seq: i as i64,
            name: name.into(),
            ir_before: Some(before),
            ir_after: Some(after),
            start_ns: (i as i64) * 1_000_000,
            end_ns: (i as i64 + 1) * 1_000_000,
            ir_changed: true,
        })?;

        let before_base = 0x1000 + (i as i64) * 0x100;
        let after_base = before_base + 0x80;
        let subject = index_side(&mut w, pass, Side::Before, FULL_STAGES[before_s], before_base, before_needle)?;
        let replacement = index_side(&mut w, pass, Side::After, FULL_STAGES[after_s], after_base, after_needle)?;

        w.write_identity_event(&IdentityEvent {
            pass,
            kind,
            ptr_token: subject,
            new_token: if kind == IdentityKind::Replaced { Some(replacement) } else { None },
            pattern: pattern.map(str::to_string),
            source: IdentitySource::Listener,
            seq: 0,
        })?;
    }

    w.finish()
}
```

- [ ] **Step 4: Run the fixture test**

Run: `cargo test -p trace-format --test identity full_fixture_has_scripted_identity_stream`
Expected: PASS.

- [ ] **Step 5: Add the `--full` CLI flag**

In `crates/cli/src/main.rs`, change the `GenFixture` variant to take a flag and dispatch:

Replace the `GenFixture` variant:

```rust
/// Write a deterministic demo trace (for development and tests)
GenFixture {
    file: PathBuf,
    /// Emit a Fidelity::Full trace with a scripted op-identity stream.
    #[arg(long)]
    full: bool,
},
```

Replace the `GenFixture` match arm:

```rust
Cmd::Dev {
    command: DevCmd::GenFixture { file, full },
} => {
    if full {
        fixture::write_full_demo_trace(&file)?;
    } else {
        fixture::write_demo_trace(&file)?;
    }
    println!("wrote {}", file.display());
    Ok(())
}
```

- [ ] **Step 6: Verify the CLI end-to-end**

Run:
```bash
cargo run -p cli -- dev gen-fixture --full /tmp/full.mlirtrace
cargo run -p cli -- trace dump /tmp/full.mlirtrace
```
Expected: `wrote /tmp/full.mlirtrace`, then a dump showing `fidelity = full` and the `canonicalize / dce / set-attr` passes.

- [ ] **Step 7: Run the workspace and commit**

Run: `cargo test --workspace && cargo clippy --workspace` — clean.

```bash
git add crates/trace-format/src/fixture.rs crates/cli/src/main.rs crates/trace-format/tests/identity.rs
git commit -m "feat: synthetic --full identity fixture and CLI flag"
```

---

## Task 4: C++ `Fidelity::Full` + storage schema v2 mirror

Mirror the schema bump and add the C++ storage write paths, plus the `Full` fidelity level. Deterministic (no lifecycle hooks yet): a `Full` run behaves like `Text` for snapshots but writes `fidelity = full` and has the identity tables available. **Requires the MLIR toolchain.**

**Files:**
- Modify: `capture/include/mlir-trace/TraceRecorder.h` (add `Fidelity::Full`)
- Modify: `capture/lib/TraceStorage.cpp` (schema DDL + version + write methods)
- Modify: `capture/lib/TraceStorage.h` (declare write methods)
- Modify: `capture/lib/TraceRecorder.cpp` (fidelity meta string)
- Modify: `capture/tests/TraceStorageTest.cpp` (assert v2 + identity round-trip)

**Interfaces:**
- Produces:
  - `enum class Fidelity { Timeline, Text, Full };`
  - `TraceStorage::writeOpIndex(int64_t passId, int side, int64_t ptrToken, int64_t byteStart, int64_t byteEnd, llvm::StringRef opName) -> llvm::Error`
  - `TraceStorage::writeIdentityEvent(int64_t passId, llvm::StringRef kind, int64_t ptrToken, std::optional<int64_t> newToken, std::optional<llvm::StringRef> pattern, llvm::StringRef source, int64_t seq) -> llvm::Error`
  - `format_version` meta = `"2"`.

- [ ] **Step 1: Write the failing test**

Append assertions to `capture/tests/TraceStorageTest.cpp` (a storage-level contract test; follows the existing `scalarEquals` helper style). After the existing checks, add — inside the test's validation — that the version is 2 and a written identity row round-trips. Add this block near where the storage test validates its trace (adapt to the file's structure; the test drives `TraceStorage` directly):

```cpp
// schema v2: format_version bumped and identity tables present + writable.
if (llvm::Error e = storage->writeOpIndex(passId.value, /*side=*/1,
                                          /*ptr=*/4096, /*start=*/0,
                                          /*end=*/12, "arith.constant"))
  return fail(std::move(e));
if (llvm::Error e = storage->writeIdentityEvent(
        passId.value, "erased", /*ptr=*/4096, std::nullopt,
        std::optional<llvm::StringRef>("DCE"), "listener", /*seq=*/0))
  return fail(std::move(e));
```

And after reopening the DB read-only, assert:

```cpp
valid = valid &&
        scalarEquals(database, "SELECT value FROM meta WHERE key='format_version'", "2") &&
        scalarEquals(database, "SELECT count(*) FROM op_index WHERE op_name='arith.constant'", 1) &&
        scalarEquals(database, "SELECT count(*) FROM op_identity WHERE kind='erased' AND source='listener'", 1);
```

(If `TraceStorageTest.cpp` does not currently retain a `passId`, capture the `PassId` returned by its `beginPass` call so the new rows can reference it.)

- [ ] **Step 2: Configure + build + run to verify it fails**

```bash
export MLIR_DIR=/Users/sungjin/work/mlir-release/lib/cmake/mlir
export LLVM_DIR=/Users/sungjin/work/mlir-release/lib/cmake/llvm
cmake -S capture -B build/capture -G Ninja -DMLIR_DIR="$MLIR_DIR" -DBUILD_TESTING=ON
cmake --build build/capture --target mlir-trace-storage-test
ctest --test-dir build/capture -R storage_contract --output-on-failure
```
Expected: build FAILS (`writeOpIndex` undeclared) — the compile error is the RED signal.

- [ ] **Step 3: Add `Fidelity::Full`**

In `capture/include/mlir-trace/TraceRecorder.h`, change:

```cpp
enum class Fidelity { Timeline, Text, Full };
```

- [ ] **Step 4: Mirror schema + bump version in `TraceStorage.cpp`**

Append the two `CREATE TABLE` + index statements (identical to the "SQL Schema Added" block above) to the `schemaSql` raw literal, before the closing `)sql"`. Change the create call comment/label and the version:

```cpp
if (llvm::Error error = execute(database, schemaSql, "create v2 schema"))
  return std::move(error);
if (llvm::Error error = storage->setMeta("format_version", "2"))
  return std::move(error);
```

- [ ] **Step 5: Declare + implement the write methods**

In `capture/lib/TraceStorage.h`, add to the `public:` section (after `endPass`):

```cpp
llvm::Error writeOpIndex(int64_t passId, int side, int64_t ptrToken,
                         int64_t byteStart, int64_t byteEnd,
                         llvm::StringRef opName);
llvm::Error writeIdentityEvent(int64_t passId, llvm::StringRef kind,
                               int64_t ptrToken,
                               std::optional<int64_t> newToken,
                               std::optional<llvm::StringRef> pattern,
                               llvm::StringRef source, int64_t seq);
```

In `capture/lib/TraceStorage.cpp`, implement them (reusing the existing `Statement`, `bindText`, `bindOptionalInt`, `makeSqliteError` helpers). Add before `TraceStorage::finish`:

```cpp
llvm::Error TraceStorage::writeOpIndex(int64_t passId, int side,
                                       int64_t ptrToken, int64_t byteStart,
                                       int64_t byteEnd,
                                       llvm::StringRef opName) {
  auto stmtOr = Statement::prepare(
      database,
      "INSERT INTO op_index "
      "(pass_id, side, ptr_token, byte_start, byte_end, op_name) "
      "VALUES (?1, ?2, ?3, ?4, ?5, ?6)");
  if (!stmtOr)
    return stmtOr.takeError();
  auto stmt = std::move(*stmtOr);
  if (sqlite3_bind_int64(stmt->get(), 1, passId) != SQLITE_OK ||
      sqlite3_bind_int(stmt->get(), 2, side) != SQLITE_OK ||
      sqlite3_bind_int64(stmt->get(), 3, ptrToken) != SQLITE_OK ||
      sqlite3_bind_int64(stmt->get(), 4, byteStart) != SQLITE_OK ||
      sqlite3_bind_int64(stmt->get(), 5, byteEnd) != SQLITE_OK)
    return makeSqliteError(database, "bind op_index");
  if (llvm::Error error = bindText(database, stmt->get(), 6, opName))
    return error;
  if (sqlite3_step(stmt->get()) != SQLITE_DONE)
    return makeSqliteError(database, "insert op_index");
  return llvm::Error::success();
}

llvm::Error TraceStorage::writeIdentityEvent(
    int64_t passId, llvm::StringRef kind, int64_t ptrToken,
    std::optional<int64_t> newToken, std::optional<llvm::StringRef> pattern,
    llvm::StringRef source, int64_t seq) {
  auto stmtOr = Statement::prepare(
      database,
      "INSERT INTO op_identity "
      "(pass_id, kind, ptr_token, new_token, pattern, source, seq) "
      "VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)");
  if (!stmtOr)
    return stmtOr.takeError();
  auto stmt = std::move(*stmtOr);
  if (sqlite3_bind_int64(stmt->get(), 1, passId) != SQLITE_OK)
    return makeSqliteError(database, "bind identity pass");
  if (llvm::Error error = bindText(database, stmt->get(), 2, kind))
    return error;
  if (sqlite3_bind_int64(stmt->get(), 3, ptrToken) != SQLITE_OK)
    return makeSqliteError(database, "bind identity token");
  if (llvm::Error error = bindOptionalInt(database, stmt->get(), 4, newToken))
    return error;
  if (pattern) {
    if (llvm::Error error = bindText(database, stmt->get(), 5, *pattern))
      return error;
  } else if (sqlite3_bind_null(stmt->get(), 5) != SQLITE_OK) {
    return makeSqliteError(database, "bind identity pattern");
  }
  if (llvm::Error error = bindText(database, stmt->get(), 6, source))
    return error;
  if (sqlite3_bind_int64(stmt->get(), 7, seq) != SQLITE_OK)
    return makeSqliteError(database, "bind identity seq");
  if (sqlite3_step(stmt->get()) != SQLITE_DONE)
    return makeSqliteError(database, "insert op_identity");
  return llvm::Error::success();
}
```

- [ ] **Step 6: Update the fidelity meta string in `TraceRecorder.cpp`**

In `attach`, replace the fidelity meta write with a three-way mapping:

```cpp
const char *fidelityName = "text";
if (options.fidelity == Fidelity::Timeline)
  fidelityName = "timeline";
else if (options.fidelity == Fidelity::Full)
  fidelityName = "full";
if (llvm::Error error = storage->setMeta("fidelity", fidelityName))
  return error;
```

Also, in `snapshot`, keep the existing early-return only for `Fidelity::Timeline` (unchanged — `Full` snapshots text like `Text`).

- [ ] **Step 7: Build + run to verify it passes**

```bash
cmake --build build/capture --target mlir-trace-storage-test
ctest --test-dir build/capture -R storage_contract --output-on-failure
```
Expected: PASS. Then rebuild the recorder tests and confirm the existing recorder tests are unaffected:
```bash
cmake --build build/capture
ctest --test-dir build/capture -R 'recorder_timeline|recorder_text|recorder_failed_pass|recorder_nested_pipeline' --output-on-failure
```
Expected: all PASS.

- [ ] **Step 8: Commit**

```bash
git add capture/include/mlir-trace/TraceRecorder.h capture/lib/TraceStorage.h capture/lib/TraceStorage.cpp capture/lib/TraceRecorder.cpp capture/tests/TraceStorageTest.cpp
git commit -m "feat(capture): Fidelity::Full and schema v2 storage methods"
```

---

## Task 5: Identity capture spike — listener + actions + op_index

**This is the time-boxed research spike (parent spec §9) — the milestone's #1 risk.** Install a `RewriterBase::Listener` and an `Actions` handler, record op byte-index at snapshot time, translate lifecycle notifications into `op_identity` events, and write a findings doc. The **acceptance bar** is: capture `inserted`/`erased`/`replaced`/`modified` reliably on a real pattern-driver pass; pattern attribution is best-effort. If precise byte offsets prove infeasible in the time-box, fall back to the documented ordinal scheme (below) and record that decision.

> **Implementer note:** This task is exploratory. If, after genuine effort, the listener does not surface events for the chosen pass, or byte-offset extraction is impractical, **report `DONE_WITH_CONCERNS` or `NEEDS_CONTEXT`** with what you observed — do not fabricate passing behavior. The findings doc is a first-class deliverable whatever the outcome.

**Files:**
- Create: `docs/superpowers/specs/M4a-identity-spike-findings.md`
- Modify: `capture/lib/TraceRecorder.cpp` (listener, action handler, op_index walk, event emission)
- Modify: `capture/tests/TraceRecorderTest.cpp` (add a `full` mode with a real greedy pattern-driver pass)
- Modify: `capture/CMakeLists.txt` (register `recorder_full` test; link `MLIRTransforms`, `MLIRTransformUtils`, `MLIRArithDialect` as needed)

**Interfaces:**
- Consumes: `TraceStorage::writeOpIndex`, `TraceStorage::writeIdentityEvent` (Task 4), `Fidelity::Full`.
- Produces: at `Fidelity::Full`, each pass with a listening rewriter writes `op_identity` rows (keyed by intra-pass `Operation*` token) and every snapshot writes `op_index` rows for its ops.

### op_index approach (with fallback)

The `ptr_token` is `reinterpret_cast<int64_t>(op)` for the `Operation*` — opaque, intra-pass only. For byte spans:

- **Preferred:** print the op tree with `Operation::print`, and for each op obtain its span by printing that single op to a scratch buffer and recording its length, accumulating offsets in pre-order. Precise spans require the same `OpPrintingFlags` as the snapshot. If a robust offset map is achievable, store real `byte_start`/`byte_end`.
- **Fallback (documented):** if precise offsets are impractical, store the op's **pre-order ordinal** in `byte_start`, set `byte_end = -1`, and note in the findings doc that M4b maps ordinals onto the M3 tolerant-parser op order (which is also pre-order). Either way `op_index` has one row per op with a stable ordering.

- [ ] **Step 1: Write the failing test — `full` recorder mode**

In `capture/tests/TraceRecorderTest.cpp`, add a mode `"full"` to `runRecorder` that builds a module with a foldable/rewritable op and runs a **real** pattern-driver pass so the listener fires. Use the canonicalizer on arith:

Add includes:
```cpp
#include "mlir/Dialect/Arith/IR/Arith.h"
#include "mlir/Parser/Parser.h"
#include "mlir/Transforms/Passes.h"
```

Add a builder that parses a rewritable module:
```cpp
mlir::OwningOpRef<mlir::ModuleOp> createFoldableModule(mlir::MLIRContext &context) {
  context.getOrLoadDialect<mlir::arith::ArithDialect>();
  context.getOrLoadDialect<mlir::func::FuncDialect>();
  constexpr llvm::StringLiteral src = R"mlir(
    module {
      func.func @f(%a: i32) -> i32 {
        %c0 = arith.constant 0 : i32
        %0 = arith.addi %a, %c0 : i32
        return %0 : i32
      }
    })mlir";
  return mlir::parseSourceString<mlir::ModuleOp>(src, &context);
}
```

In `runRecorder`, handle `mode == "full"`: build the foldable module, add `mlir::createCanonicalizerPass()` (nested on `func::FuncOp`), set `Fidelity::Full`, run. After running, the validation in `main` for `"full"` asserts:

```cpp
} else if (mode == "full") {
  valid = valid &&
          scalarEquals(database, "SELECT value FROM meta WHERE key='fidelity'", "full") &&
          // op_index populated for the canonicalize pass snapshots
          scalarEquals(database,
                       "SELECT (SELECT count(*) FROM op_index) > 0", 1) &&
          // at least one lifecycle event was captured
          scalarEquals(database,
                       "SELECT (SELECT count(*) FROM op_identity) > 0", 1);
}
```

Register the mode in the arg validation (`mode != "full"` added to the allowed set) and add the builder branch.

- [ ] **Step 2: Register the test in CMake and build to verify it fails**

In `capture/CMakeLists.txt`, add to the recorder test's `target_link_libraries` the transform/dialect libs, and register the test:

```cmake
target_link_libraries(mlir-trace-recorder-test PRIVATE
  MLIRTrace MLIRFuncDialect MLIRArithDialect MLIRTransforms SQLite::SQLite3)

add_test(NAME recorder_full
  COMMAND mlir-trace-recorder-test
          full
          "${CMAKE_CURRENT_BINARY_DIR}/recorder-full.mlirtrace")
```

Build + run:
```bash
cmake -S capture -B build/capture -G Ninja -DMLIR_DIR="$MLIR_DIR" -DBUILD_TESTING=ON
cmake --build build/capture --target mlir-trace-recorder-test
ctest --test-dir build/capture -R recorder_full --output-on-failure
```
Expected: FAIL — `op_index` and `op_identity` are empty because the recorder doesn't yet emit them.

- [ ] **Step 3: Implement op_index walk + listener + action handler in `TraceRecorder.cpp`**

Add includes:
```cpp
#include "mlir/IR/PatternMatch.h"      // RewriterBase::Listener
#include "mlir/IR/Action.h"           // tracing::Action
#include "mlir/IR/Operation.h"
```

Add a nested listener that forwards notifications to the `Impl`, storing events against the currently-active pass. Key design points to implement:
- A `RewriterBase::Listener` subclass whose `notifyOperationInserted`, `notifyOperationErased`, `notifyOperationReplaced(Operation*, ValueRange/Operation*)`, `notifyOperationModified` call back into `Impl` with the op pointer(s) and current pass id.
- Register the listener on the context so pattern drivers pick it up. In LLVM 21, attach via the pattern driver's `GreedyRewriteConfig.listener` where reachable; where a context-wide listener is not available, document the limitation (spike finding). Register an action handler with `context.registerActionHandler(...)` that records the current pattern/action name into `Impl` so concurrent events get `pattern` + `source = action`; listener-only events get `source = listener`.
- In `afterPass` (at `Fidelity::Full`), after taking the `after` snapshot, walk the operation tree in pre-order and call `storage->writeOpIndex(passId, side, token, start, end, opName)` for both the before and after snapshots (before-walk in `beforePass`, after-walk in `afterPass`).
- Emit accumulated `op_identity` events for the pass via `storage->writeIdentityEvent(...)`, numbering `seq` from 0 in observation order.

Implement to the acceptance bar; if a notification hook is unavailable, capture the subset that is and record the gap. (Full code is the implementer's to write from the MLIR 21 headers — this is the spike.)

- [ ] **Step 4: Build + run until the acceptance test passes**

```bash
cmake --build build/capture --target mlir-trace-recorder-test
ctest --test-dir build/capture -R recorder_full --output-on-failure
```
Expected: PASS — `op_index` non-empty and ≥1 `op_identity` event. Also re-run the other recorder tests to confirm no regression:
```bash
ctest --test-dir build/capture -R 'recorder_' --output-on-failure
```

- [ ] **Step 5: Write the findings doc**

Create `docs/superpowers/specs/M4a-identity-spike-findings.md` recording, against LLVM/MLIR 21.1: which notifications fired for the canonicalizer (and any other pass tried), whether the `Actions` handler yielded pattern names, which lifecycle kinds are reliably captured vs. absent, the op_index approach chosen (precise offsets vs. ordinal fallback) and why, and the residual gaps M4b's fingerprint matcher must cover. Keep it to one page.

- [ ] **Step 6: Commit**

```bash
git add capture/lib/TraceRecorder.cpp capture/tests/TraceRecorderTest.cpp capture/CMakeLists.txt docs/superpowers/specs/M4a-identity-spike-findings.md
git commit -m "feat(capture): identity spike — listener/actions capture + op_index"
```

---

## Task 6: Full verification + branch finish

Verify both halves together and finish the branch.

**Files:** none (verification + docs/ledger only).

- [ ] **Step 1: Rust workspace green**

```bash
cargo test --workspace
cargo clippy --workspace
cargo fmt --check
```
Expected: all tests pass, clippy clean, fmt clean.

- [ ] **Step 2: C++ suite green**

```bash
export MLIR_DIR=/Users/sungjin/work/mlir-release/lib/cmake/mlir
export LLVM_DIR=/Users/sungjin/work/mlir-release/lib/cmake/llvm
cmake --build build/capture
ctest --test-dir build/capture --output-on-failure
```
Expected: all C++ tests pass (storage, recorder incl. `recorder_full`, rust-reader cross-check, install-consumer).

- [ ] **Step 3: Cross-language sanity**

Confirm a C++-produced `Full` trace opens in the Rust reader with identity rows:
```bash
./build/capture/capture-toy/mlir-trace-example /tmp/cpp-full.mlirtrace  # if toy updated to Full; else use recorder-full output
cargo run -p cli -- trace dump /tmp/cpp-full.mlirtrace
```
Expected: dump shows `fidelity` and passes; no reader error. (If the toy still runs `Text`, use `build/capture/recorder-full.mlirtrace` from Task 5.)

- [ ] **Step 4: Update the SDD ledger and finish the branch**

Record completion in the SDD progress ledger, then use the `superpowers:finishing-a-development-branch` skill to merge `feat/m4a-op-identity-capture` into `main`.

---

## Self-Review notes (for the author)

- **Spec §4 (C++ capture):** Tasks 4–5. **§5 (schema v2):** Tasks 1–2 (Rust), Task 4 (C++ mirror). **§6 (fixture):** Task 3. **§8 edge cases** (non-Full → no tables; v1 open; no-rewriter pass → zero events): covered by Task 1 `v1_trace_still_opens`, Task 2 `identity_accessors_empty_on_v1`, and the Task 5 acceptance/findings. **§9 testing:** Rust round-trip + back-compat (Tasks 1–3), C++ recorder `full` mode (Task 5).
- **Type consistency:** `OpIndexRow` / `IdentityEvent` / `Side` / `IdentityKind` / `IdentitySource` defined in Task 2 `identity.rs`, used unchanged in Task 3 fixture and Task 6. C++ `writeOpIndex` / `writeIdentityEvent` signatures defined in Task 4, called in Task 5. String encodings (`side` 0/1, `kind`, `source`) are identical across Rust `as_str`/`from_str` and C++ string literals.
- **Deferred correctly:** no provenance engine, no uid resolution, no API, no UI (all M4b).
