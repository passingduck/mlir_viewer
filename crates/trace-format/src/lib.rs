pub mod error;
pub mod schema;
pub mod writer;

pub use error::{Result, TraceError};
pub use writer::{BlobId, PassId, PassRecord, TraceWriter};

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use crate::writer::{PassRecord, TraceWriter};

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
}
