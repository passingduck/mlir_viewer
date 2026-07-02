# M0 — Trace Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Contract 1 of the MLIR Viewer — the versioned SQLite trace format with a Rust writer/reader, a synthetic fixture generator, and a `mlir-viewer trace dump` CLI — fully testable without any compiler.

**Architecture:** Cargo workspace with two crates: `trace-format` (schema, writer, reader, fixture) and `cli` (the `mlir-viewer` binary). The schema is spec §5 verbatim. The C++ capture library (M1) and the server (M2) build on exactly these artifacts.

**Tech Stack:** Rust (edition 2021), rusqlite (bundled SQLite), zstd, xxhash-rust, clap, anyhow, thiserror.

## Global Constraints

- Trace format version: `format_version = "1"` stored in `meta`; readers must reject other major versions with a clear error.
- Blob compression: zstd level 3; `compression` column value is exactly `"zstd"`.
- Blob hash: xxhash3-64 of the **uncompressed** text, stored as 8-byte big-endian BLOB.
- Timestamps are `i64` nanoseconds (monotonic origin arbitrary; only deltas matter).
- All SQL lives in `crates/trace-format/src/schema.rs`; no inline SQL strings elsewhere except queries in `reader.rs`/`writer.rs`.
- rusqlite must use the `bundled` feature (no system SQLite dependency).
- Binary name is `mlir-viewer`.
- Every task ends with `cargo test --workspace` green and a commit.

---

### Task 1: Workspace scaffold + schema module

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/trace-format/Cargo.toml`
- Create: `crates/trace-format/src/lib.rs`
- Create: `crates/trace-format/src/schema.rs`
- Create: `.gitignore`

**Interfaces:**
- Consumes: nothing (first task).
- Produces: `trace_format::schema::{SCHEMA_SQL, FORMAT_VERSION}` — `pub const FORMAT_VERSION: &str = "1"`, `pub const SCHEMA_SQL: &str` (DDL). Later tasks call `Connection::execute_batch(SCHEMA_SQL)`.

- [ ] **Step 1: Create workspace and crate manifests**

`Cargo.toml` (root):

```toml
[workspace]
resolver = "2"
members = ["crates/trace-format", "crates/cli"]

[workspace.dependencies]
anyhow = "1"
thiserror = "2"
rusqlite = { version = "0.32", features = ["bundled"] }
zstd = "0.13"
xxhash-rust = { version = "0.8", features = ["xxh3"] }
clap = { version = "4", features = ["derive"] }
```

`.gitignore`:

```
/target
*.mlirtrace
```

`crates/trace-format/Cargo.toml`:

```toml
[package]
name = "trace-format"
version = "0.1.0"
edition = "2021"

[dependencies]
anyhow = { workspace = true }
thiserror = { workspace = true }
rusqlite = { workspace = true }
zstd = { workspace = true }
xxhash-rust = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

(Note: `crates/cli` is added to `members` now but created in Task 5; until then run cargo commands with `-p trace-format`.)

- [ ] **Step 2: Write the failing test**

`crates/trace-format/src/lib.rs`:

```rust
pub mod schema;

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    #[test]
    fn schema_applies_cleanly() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::schema::SCHEMA_SQL).unwrap();
        // All three core tables exist.
        for table in ["meta", "ir_blob", "pass_execution"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing table {table}");
        }
        assert_eq!(crate::schema::FORMAT_VERSION, "1");
    }
}
```

Create `crates/trace-format/src/schema.rs` as an empty file so the module resolves but constants are missing.

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p trace-format`
Expected: FAIL — `cannot find value SCHEMA_SQL` (compile error).

- [ ] **Step 4: Implement the schema module**

`crates/trace-format/src/schema.rs`:

```rust
/// Trace format major version. Readers reject files with a different value.
pub const FORMAT_VERSION: &str = "1";

/// Full DDL for a v1 trace file. Matches design spec §5.
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE ir_blob (
    id          INTEGER PRIMARY KEY,
    hash        BLOB NOT NULL UNIQUE,
    size_bytes  INTEGER NOT NULL,
    compression TEXT NOT NULL,
    data        BLOB NOT NULL
);

CREATE TABLE pass_execution (
    id         INTEGER PRIMARY KEY,
    parent_id  INTEGER REFERENCES pass_execution(id),
    seq        INTEGER NOT NULL,
    name       TEXT NOT NULL,
    ir_before  INTEGER REFERENCES ir_blob(id),
    ir_after   INTEGER REFERENCES ir_blob(id),
    start_ns   INTEGER NOT NULL,
    end_ns     INTEGER NOT NULL,
    ir_changed INTEGER NOT NULL
);

CREATE INDEX idx_pass_parent ON pass_execution(parent_id, seq);
"#;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p trace-format`
Expected: PASS (1 test).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml .gitignore crates/trace-format
git commit -m "feat(trace-format): workspace scaffold and schema v1"
```

---

### Task 2: TraceWriter with content-addressed blob dedup

**Files:**
- Create: `crates/trace-format/src/writer.rs`
- Create: `crates/trace-format/src/error.rs`
- Modify: `crates/trace-format/src/lib.rs`

**Interfaces:**
- Consumes: `schema::{SCHEMA_SQL, FORMAT_VERSION}` from Task 1.
- Produces:
  - `TraceError` (thiserror enum), `pub type Result<T> = std::result::Result<T, TraceError>;`
  - `BlobId(pub i64)`, `PassId(pub i64)` — newtype wrappers, `#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]`.
  - `TraceWriter::create(path: &Path) -> Result<TraceWriter>`
  - `TraceWriter::set_meta(&mut self, key: &str, value: &str) -> Result<()>`
  - `TraceWriter::write_blob(&mut self, text: &str) -> Result<BlobId>` — dedups by hash.
  - `TraceWriter::record_pass(&mut self, rec: &PassRecord) -> Result<PassId>`
  - `PassRecord { pub parent: Option<PassId>, pub seq: i64, pub name: String, pub ir_before: Option<BlobId>, pub ir_after: Option<BlobId>, pub start_ns: i64, pub end_ns: i64, pub ir_changed: bool }`
  - `TraceWriter::finish(self) -> Result<()>`

- [ ] **Step 1: Write the failing tests**

Append to `crates/trace-format/src/lib.rs`:

```rust
pub mod error;
pub mod writer;

pub use error::{Result, TraceError};
pub use writer::{BlobId, PassId, PassRecord, TraceWriter};
```

Add to the `tests` module in `lib.rs`:

```rust
    use crate::writer::{PassRecord, TraceWriter};

    #[test]
    fn writer_creates_valid_file_with_meta() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.mlirtrace");
        let mut w = TraceWriter::create(&path).unwrap();
        w.set_meta("producer", "test").unwrap();
        w.finish().unwrap();

        let conn = Connection::open(&path).unwrap();
        let v: String = conn
            .query_row("SELECT value FROM meta WHERE key='format_version'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "1");
        let p: String = conn
            .query_row("SELECT value FROM meta WHERE key='producer'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(p, "test");
    }

    #[test]
    fn identical_blobs_are_deduplicated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.mlirtrace");
        let mut w = TraceWriter::create(&path).unwrap();
        let a = w.write_blob("module {}").unwrap();
        let b = w.write_blob("module {}").unwrap();
        let c = w.write_blob("module {func.func @f()}").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
        w.finish().unwrap();

        let conn = Connection::open(&path).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM ir_blob", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn record_pass_round_trips_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.mlirtrace");
        let mut w = TraceWriter::create(&path).unwrap();
        let before = w.write_blob("module { A }").unwrap();
        let after = w.write_blob("module { B }").unwrap();
        let root = w
            .record_pass(&PassRecord {
                parent: None, seq: 0, name: "Pipeline".into(),
                ir_before: Some(before), ir_after: Some(after),
                start_ns: 100, end_ns: 900, ir_changed: true,
            })
            .unwrap();
        w.record_pass(&PassRecord {
                parent: Some(root), seq: 0, name: "canonicalize".into(),
                ir_before: Some(before), ir_after: Some(after),
                start_ns: 150, end_ns: 400, ir_changed: true,
            })
            .unwrap();
        w.finish().unwrap();

        let conn = Connection::open(&path).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM pass_execution", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        let child_parent: i64 = conn
            .query_row("SELECT parent_id FROM pass_execution WHERE name='canonicalize'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(child_parent, root.0);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p trace-format`
Expected: FAIL — modules `error`/`writer` don't exist (compile error).

- [ ] **Step 3: Implement error and writer**

`crates/trace-format/src/error.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported trace format version {found} (reader supports {supported})")]
    VersionMismatch { found: String, supported: &'static str },
    #[error("corrupt trace: {0}")]
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, TraceError>;
```

`crates/trace-format/src/writer.rs`:

```rust
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use xxhash_rust::xxh3::xxh3_64;

use crate::error::Result;
use crate::schema::{FORMAT_VERSION, SCHEMA_SQL};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlobId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PassId(pub i64);

#[derive(Debug, Clone)]
pub struct PassRecord {
    pub parent: Option<PassId>,
    pub seq: i64,
    pub name: String,
    pub ir_before: Option<BlobId>,
    pub ir_after: Option<BlobId>,
    pub start_ns: i64,
    pub end_ns: i64,
    pub ir_changed: bool,
}

pub struct TraceWriter {
    conn: Connection,
}

impl TraceWriter {
    pub fn create(path: &Path) -> Result<TraceWriter> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA_SQL)?;
        let mut w = TraceWriter { conn };
        w.set_meta("format_version", FORMAT_VERSION)?;
        Ok(w)
    }

    pub fn set_meta(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Store `text` content-addressed; returns the existing id on duplicate content.
    pub fn write_blob(&mut self, text: &str) -> Result<BlobId> {
        let hash = xxh3_64(text.as_bytes()).to_be_bytes();
        if let Some(id) = self
            .conn
            .query_row("SELECT id FROM ir_blob WHERE hash = ?1", params![&hash[..]], |r| r.get(0))
            .optional()?
        {
            return Ok(BlobId(id));
        }
        let compressed = zstd::encode_all(text.as_bytes(), 3)?;
        self.conn.execute(
            "INSERT INTO ir_blob(hash, size_bytes, compression, data) VALUES (?1, ?2, 'zstd', ?3)",
            params![&hash[..], text.len() as i64, compressed],
        )?;
        Ok(BlobId(self.conn.last_insert_rowid()))
    }

    pub fn record_pass(&mut self, rec: &PassRecord) -> Result<PassId> {
        self.conn.execute(
            "INSERT INTO pass_execution
             (parent_id, seq, name, ir_before, ir_after, start_ns, end_ns, ir_changed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                rec.parent.map(|p| p.0),
                rec.seq,
                rec.name,
                rec.ir_before.map(|b| b.0),
                rec.ir_after.map(|b| b.0),
                rec.start_ns,
                rec.end_ns,
                rec.ir_changed as i64,
            ],
        )?;
        Ok(PassId(self.conn.last_insert_rowid()))
    }

    pub fn finish(self) -> Result<()> {
        self.conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p trace-format`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/trace-format
git commit -m "feat(trace-format): TraceWriter with content-addressed blob dedup"
```

---

### Task 3: TraceReader with version validation

**Files:**
- Create: `crates/trace-format/src/reader.rs`
- Modify: `crates/trace-format/src/lib.rs`

**Interfaces:**
- Consumes: Task 1 constants, Task 2 types (`BlobId`, `PassId`, `TraceError`, writer for test setup).
- Produces:
  - `TraceReader::open(path: &Path) -> Result<TraceReader>` — errors with `TraceError::VersionMismatch` on wrong `format_version`, `TraceError::Corrupt` if the meta key is absent.
  - `TraceReader::meta(&self) -> Result<std::collections::BTreeMap<String, String>>`
  - `TraceReader::passes(&self) -> Result<Vec<PassNode>>` — forest of root passes, children ordered by `seq`.
  - `PassNode { pub id: PassId, pub name: String, pub ir_before: Option<BlobId>, pub ir_after: Option<BlobId>, pub start_ns: i64, pub end_ns: i64, pub ir_changed: bool, pub children: Vec<PassNode> }`
  - `TraceReader::blob_text(&self, id: BlobId) -> Result<String>` — decompresses and verifies hash.

- [ ] **Step 1: Write the failing tests**

Add to `lib.rs`:

```rust
pub mod reader;
pub use reader::{PassNode, TraceReader};
```

Add tests:

```rust
    use crate::reader::TraceReader;
    use crate::TraceError;

    fn write_two_pass_trace(path: &std::path::Path) -> (crate::PassId, crate::BlobId) {
        let mut w = TraceWriter::create(path).unwrap();
        w.set_meta("producer", "test").unwrap();
        let before = w.write_blob("module { A }").unwrap();
        let after = w.write_blob("module { B }").unwrap();
        let root = w
            .record_pass(&PassRecord {
                parent: None, seq: 0, name: "Pipeline".into(),
                ir_before: Some(before), ir_after: Some(after),
                start_ns: 0, end_ns: 1000, ir_changed: true,
            })
            .unwrap();
        w.record_pass(&PassRecord {
                parent: Some(root), seq: 0, name: "cse".into(),
                ir_before: Some(before), ir_after: Some(after),
                start_ns: 10, end_ns: 500, ir_changed: true,
            })
            .unwrap();
        w.finish().unwrap();
        (root, before)
    }

    #[test]
    fn reader_round_trips_pass_tree_and_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.mlirtrace");
        let (_root, before) = write_two_pass_trace(&path);

        let r = TraceReader::open(&path).unwrap();
        assert_eq!(r.meta().unwrap()["producer"], "test");
        let roots = r.passes().unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].name, "Pipeline");
        assert_eq!(roots[0].children.len(), 1);
        assert_eq!(roots[0].children[0].name, "cse");
        assert_eq!(r.blob_text(before).unwrap(), "module { A }");
    }

    #[test]
    fn reader_rejects_wrong_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.mlirtrace");
        write_two_pass_trace(&path);
        let conn = Connection::open(&path).unwrap();
        conn.execute("UPDATE meta SET value='99' WHERE key='format_version'", []).unwrap();
        drop(conn);

        match TraceReader::open(&path) {
            Err(TraceError::VersionMismatch { found, .. }) => assert_eq!(found, "99"),
            other => panic!("expected VersionMismatch, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p trace-format`
Expected: FAIL — module `reader` doesn't exist (compile error).

- [ ] **Step 3: Implement the reader**

`crates/trace-format/src/reader.rs`:

```rust
use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use xxhash_rust::xxh3::xxh3_64;

use crate::error::{Result, TraceError};
use crate::schema::FORMAT_VERSION;
use crate::writer::{BlobId, PassId};

#[derive(Debug)]
pub struct PassNode {
    pub id: PassId,
    pub name: String,
    pub ir_before: Option<BlobId>,
    pub ir_after: Option<BlobId>,
    pub start_ns: i64,
    pub end_ns: i64,
    pub ir_changed: bool,
    pub children: Vec<PassNode>,
}

pub struct TraceReader {
    conn: Connection,
}

impl TraceReader {
    pub fn open(path: &Path) -> Result<TraceReader> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let version: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key='format_version'", [], |r| r.get(0))
            .optional()?;
        match version {
            None => return Err(TraceError::Corrupt("missing format_version".into())),
            Some(v) if v != FORMAT_VERSION => {
                return Err(TraceError::VersionMismatch { found: v, supported: FORMAT_VERSION })
            }
            Some(_) => {}
        }
        Ok(TraceReader { conn })
    }

    pub fn meta(&self) -> Result<BTreeMap<String, String>> {
        let mut stmt = self.conn.prepare("SELECT key, value FROM meta")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (k, v) = row?;
            out.insert(k, v);
        }
        Ok(out)
    }

    pub fn passes(&self) -> Result<Vec<PassNode>> {
        struct Row {
            id: i64,
            parent: Option<i64>,
            node: PassNode,
        }
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_id, name, ir_before, ir_after, start_ns, end_ns, ir_changed
             FROM pass_execution ORDER BY parent_id NULLS FIRST, seq",
        )?;
        let rows: Vec<Row> = stmt
            .query_map([], |r| {
                Ok(Row {
                    id: r.get(0)?,
                    parent: r.get(1)?,
                    node: PassNode {
                        id: PassId(r.get(0)?),
                        name: r.get(2)?,
                        ir_before: r.get::<_, Option<i64>>(3)?.map(BlobId),
                        ir_after: r.get::<_, Option<i64>>(4)?.map(BlobId),
                        start_ns: r.get(5)?,
                        end_ns: r.get(6)?,
                        ir_changed: r.get::<_, i64>(7)? != 0,
                        children: Vec::new(),
                    },
                })
            })?
            .collect::<std::result::Result<_, _>>()?;

        // Assemble forest: children appear after parents (ids are insertion-ordered).
        let mut nodes: std::collections::BTreeMap<i64, PassNode> = BTreeMap::new();
        let mut parent_of: BTreeMap<i64, Option<i64>> = BTreeMap::new();
        for row in rows {
            parent_of.insert(row.id, row.parent);
            nodes.insert(row.id, row.node);
        }
        let mut roots = Vec::new();
        for (id, parent) in parent_of.iter().rev() {
            let node = nodes.remove(id).expect("node present");
            match parent {
                Some(p) => nodes
                    .get_mut(p)
                    .ok_or_else(|| TraceError::Corrupt(format!("pass {id} has unknown parent {p}")))?
                    .children
                    .insert(0, node),
                None => roots.insert(0, node),
            }
        }
        Ok(roots)
    }

    pub fn blob_text(&self, id: BlobId) -> Result<String> {
        let (hash, compression, data): (Vec<u8>, String, Vec<u8>) = self.conn.query_row(
            "SELECT hash, compression, data FROM ir_blob WHERE id = ?1",
            params![id.0],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if compression != "zstd" {
            return Err(TraceError::Corrupt(format!("unknown compression {compression}")));
        }
        let bytes = zstd::decode_all(&data[..])
            .map_err(|e| TraceError::Corrupt(format!("zstd decode failed: {e}")))?;
        if xxh3_64(&bytes).to_be_bytes()[..] != hash[..] {
            return Err(TraceError::Corrupt(format!("blob {} hash mismatch", id.0)));
        }
        String::from_utf8(bytes).map_err(|e| TraceError::Corrupt(format!("blob not utf-8: {e}")))
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p trace-format`
Expected: PASS (6 tests). If the child-assembly ordering assertion fails, note that `BTreeMap::iter().rev()` processes highest ids first — children (always inserted after their parents, hence higher ids) are detached before their parents are moved; do not "simplify" this loop.

- [ ] **Step 5: Commit**

```bash
git add crates/trace-format
git commit -m "feat(trace-format): TraceReader with version and integrity validation"
```

---

### Task 4: Synthetic fixture generator

**Files:**
- Create: `crates/trace-format/src/fixture.rs`
- Modify: `crates/trace-format/src/lib.rs`

**Interfaces:**
- Consumes: `TraceWriter`, `PassRecord` (Task 2).
- Produces: `fixture::write_demo_trace(path: &Path) -> Result<()>` — a deterministic trace resembling a torch-to-LLVM pipeline: root `Pipeline` pass with 5 children (`canonicalize`, `cse`, `my-custom-fusion`, `one-shot-bufferize`, `convert-to-llvm`), realistic MLIR-flavored text snapshots, one no-op pass (`cse` — demonstrates dedup + `ir_changed = false`). Used by CLI tests (Task 5), M2 server tests, and UI development.

- [ ] **Step 1: Write the failing test**

Add to `lib.rs`:

```rust
pub mod fixture;
```

Add test:

```rust
    #[test]
    fn demo_fixture_is_valid_and_deduped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("demo.mlirtrace");
        crate::fixture::write_demo_trace(&path).unwrap();

        let r = TraceReader::open(&path).unwrap();
        let roots = r.passes().unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].children.len(), 5);
        let cse = &roots[0].children[1];
        assert_eq!(cse.name, "cse");
        assert!(!cse.ir_changed);
        // A no-op pass shares before/after blob ids.
        assert_eq!(cse.ir_before, cse.ir_after);
        // Every snapshot decompresses to non-empty MLIR-ish text.
        let text = r.blob_text(roots[0].ir_before.unwrap()).unwrap();
        assert!(text.contains("func.func @forward"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p trace-format`
Expected: FAIL — module `fixture` doesn't exist (compile error).

- [ ] **Step 3: Implement the fixture**

`crates/trace-format/src/fixture.rs`:

```rust
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

const PASS_NAMES: [&str; 5] =
    ["canonicalize", "cse", "my-custom-fusion", "one-shot-bufferize", "convert-to-llvm"];

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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p trace-format`
Expected: PASS (7 tests). Note the dedup interplay: stages 1 and 2 are byte-identical, so `write_blob` returns the same `BlobId` and the `cse` pass records `ir_changed = false` automatically.

- [ ] **Step 5: Commit**

```bash
git add crates/trace-format
git commit -m "feat(trace-format): deterministic demo trace fixture"
```

---

### Task 5: `mlir-viewer` CLI with `trace dump` and `dev gen-fixture`

**Files:**
- Create: `crates/cli/Cargo.toml`
- Create: `crates/cli/src/main.rs`
- Create: `crates/cli/tests/trace_dump.rs`

**Interfaces:**
- Consumes: `TraceReader`, `PassNode`, `fixture::write_demo_trace` (Tasks 3–4).
- Produces: the `mlir-viewer` binary with subcommands:
  - `mlir-viewer trace dump <FILE>` — prints meta, then the pass tree with durations and change markers; exit code 1 with the `TraceError` message on invalid files.
  - `mlir-viewer dev gen-fixture <FILE>` — writes the demo trace (developer/test tool; also how M2 UI development gets data before M1 exists).

- [ ] **Step 1: Create the crate and write the failing integration test**

`crates/cli/Cargo.toml`:

```toml
[package]
name = "cli"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "mlir-viewer"
path = "src/main.rs"

[dependencies]
anyhow = { workspace = true }
clap = { workspace = true }
trace-format = { path = "../trace-format" }

[dev-dependencies]
assert_cmd = "2"
predicates = "3"
tempfile = "3"
```

`crates/cli/tests/trace_dump.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn gen_fixture_then_dump_shows_pass_tree() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");

    Command::cargo_bin("mlir-viewer")
        .unwrap()
        .args(["dev", "gen-fixture"])
        .arg(&trace)
        .assert()
        .success();

    Command::cargo_bin("mlir-viewer")
        .unwrap()
        .args(["trace", "dump"])
        .arg(&trace)
        .assert()
        .success()
        .stdout(predicate::str::contains("Pipeline"))
        .stdout(predicate::str::contains("canonicalize"))
        // no-op pass is visibly marked
        .stdout(predicate::str::contains("cse").and(predicate::str::contains("(no change)")))
        .stdout(predicate::str::contains("1.00ms"));
}

#[test]
fn dump_rejects_non_trace_file() {
    let dir = tempfile::tempdir().unwrap();
    let bogus = dir.path().join("bogus.mlirtrace");
    std::fs::write(&bogus, "not a database").unwrap();

    Command::cargo_bin("mlir-viewer")
        .unwrap()
        .args(["trace", "dump"])
        .arg(&bogus)
        .assert()
        .failure();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cli`
Expected: FAIL — `src/main.rs` missing (compile error).

- [ ] **Step 3: Implement the CLI**

`crates/cli/src/main.rs`:

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use trace_format::{fixture, PassNode, TraceReader};

#[derive(Parser)]
#[command(name = "mlir-viewer", version, about = "Visual debugger for MLIR pass pipelines")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Inspect trace files
    Trace {
        #[command(subcommand)]
        command: TraceCmd,
    },
    /// Developer utilities
    Dev {
        #[command(subcommand)]
        command: DevCmd,
    },
}

#[derive(Subcommand)]
enum TraceCmd {
    /// Print trace metadata and the pass execution tree
    Dump { file: PathBuf },
}

#[derive(Subcommand)]
enum DevCmd {
    /// Write a deterministic demo trace (for development and tests)
    GenFixture { file: PathBuf },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Cmd::Trace { command: TraceCmd::Dump { file } } => dump(&file),
        Cmd::Dev { command: DevCmd::GenFixture { file } } => {
            fixture::write_demo_trace(&file)?;
            println!("wrote {}", file.display());
            Ok(())
        }
    }
}

fn dump(file: &std::path::Path) -> Result<()> {
    let reader = TraceReader::open(file)?;
    println!("# meta");
    for (k, v) in reader.meta()? {
        println!("  {k} = {v}");
    }
    println!("# passes");
    for root in reader.passes()? {
        print_pass(&root, 0);
    }
    Ok(())
}

fn print_pass(node: &PassNode, depth: usize) {
    let indent = "  ".repeat(depth + 1);
    let ms = (node.end_ns - node.start_ns) as f64 / 1_000_000.0;
    let marker = if node.ir_changed { "" } else { "  (no change)" };
    println!("{indent}{} — {ms:.2}ms{marker}", node.name);
    for child in &node.children {
        print_pass(child, depth + 1);
    }
}
```

- [ ] **Step 4: Run the full workspace test suite**

Run: `cargo test --workspace`
Expected: PASS (7 trace-format tests + 2 cli tests). Also sanity-run by hand:

```bash
cargo run -q -p cli -- dev gen-fixture /tmp/demo.mlirtrace
cargo run -q -p cli -- trace dump /tmp/demo.mlirtrace
```

Expected output shape:

```
# meta
  created_at_utc = 2026-07-02T00:00:00Z
  format_version = 1
  producer = trace-format fixture 0.1
# passes
  Pipeline — 5.00ms
    canonicalize — 1.00ms
    cse — 1.00ms  (no change)
    my-custom-fusion — 1.00ms
    one-shot-bufferize — 1.00ms
    convert-to-llvm — 1.00ms
```

- [ ] **Step 5: Commit**

```bash
git add crates/cli
git commit -m "feat(cli): mlir-viewer binary with trace dump and dev gen-fixture"
```

---

### Task 6: Golden conformance fixture (Contract 1 anchor for M1)

**Files:**
- Create: `crates/trace-format/tests/conformance.rs`
- Create: `testdata/golden/demo-v1.mlirtrace` (generated, committed)

**Interfaces:**
- Consumes: `TraceReader`, `fixture::write_demo_trace`.
- Produces: a committed golden trace file + a test asserting the current reader accepts it. In M1 the C++ writer gains the mirror-image test (its output must satisfy this same reader). This test is the enforcement mechanism of Contract 1: any schema change that breaks old files fails CI here.

- [ ] **Step 1: Generate and commit the golden file**

```bash
mkdir -p testdata/golden
cargo run -q -p cli -- dev gen-fixture testdata/golden/demo-v1.mlirtrace
```

Remove the `*.mlirtrace` ignore collision by amending `.gitignore`:

```
/target
*.mlirtrace
!testdata/golden/*.mlirtrace
```

- [ ] **Step 2: Write the conformance test (fails only if reader regresses — write it to pass now, guard forever)**

`crates/trace-format/tests/conformance.rs`:

```rust
use trace_format::TraceReader;

/// Contract 1 anchor: v1 golden files must remain readable forever within major version 1.
/// If this test breaks, you changed the format — bump FORMAT_VERSION and write a migration
/// story instead of editing this file.
#[test]
fn golden_v1_trace_remains_readable() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata/golden/demo-v1.mlirtrace");
    let r = TraceReader::open(std::path::Path::new(path)).unwrap();
    let roots = r.passes().unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].children.len(), 5);
    let text = r.blob_text(roots[0].ir_before.unwrap()).unwrap();
    assert!(text.contains("linalg.matmul"));
}
```

- [ ] **Step 3: Run the full suite**

Run: `cargo test --workspace`
Expected: PASS (10 tests total).

- [ ] **Step 4: Commit**

```bash
git add .gitignore testdata crates/trace-format/tests
git commit -m "test(trace-format): golden v1 conformance trace anchoring Contract 1"
```

---

## Self-Review Notes

- **Spec coverage (M0 scope only, per spec §13):** schema v1 ✓ (Task 1, spec §5 verbatim), writer + dedup + zstd + xxhash ✓ (Task 2, matches Global Constraints), reader + version rejection ✓ (Task 3), fixture generator ✓ (Task 4), dump CLI ✓ (Task 5), conformance anchor for the M1 C++ writer ✓ (Task 6). Out of scope by design: structural tables, diff, server, UI (M1–M3 plans).
- **Type consistency:** `BlobId`/`PassId`/`PassRecord`/`PassNode`/`TraceError` signatures identical across Tasks 2–6 interface blocks.
- **Placeholder scan:** none; all steps carry complete code and exact commands.
