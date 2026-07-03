pub mod error;
pub mod fixture;
pub mod reader;
pub mod schema;
pub mod writer;

pub use error::{Result, TraceError};
pub use reader::{PassNode, TraceReader};
pub use writer::{BlobId, PassId, PassRecord, TraceWriter};

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use crate::reader::TraceReader;
    use crate::writer::{PassRecord, TraceWriter};
    use crate::TraceError;

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
    fn passes_orders_children_by_seq_not_insertion_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.mlirtrace");
        let mut w = TraceWriter::create(&path).unwrap();
        let blob = w.write_blob("module { A }").unwrap();
        let root = w
            .record_pass(&PassRecord {
                parent: None, seq: 0, name: "Pipeline".into(),
                ir_before: Some(blob), ir_after: Some(blob),
                start_ns: 0, end_ns: 1000, ir_changed: true,
            })
            .unwrap();
        // Record children in an order that DIVERGES from their seq values:
        // the seq-1 child is inserted first (lower id), the seq-0 child second.
        w.record_pass(&PassRecord {
                parent: Some(root), seq: 1, name: "second".into(),
                ir_before: Some(blob), ir_after: Some(blob),
                start_ns: 10, end_ns: 20, ir_changed: false,
            })
            .unwrap();
        w.record_pass(&PassRecord {
                parent: Some(root), seq: 0, name: "first".into(),
                ir_before: Some(blob), ir_after: Some(blob),
                start_ns: 30, end_ns: 40, ir_changed: false,
            })
            .unwrap();
        w.finish().unwrap();

        let r = TraceReader::open(&path).unwrap();
        let roots = r.passes().unwrap();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].children.len(), 2);
        assert_eq!(roots[0].children[0].name, "first", "children must be ordered by seq, not insertion id");
        assert_eq!(roots[0].children[1].name, "second");
    }

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
            Err(other) => panic!("expected VersionMismatch, got {other:?}"),
            Ok(_) => panic!("expected VersionMismatch, got Ok"),
        }
    }
}
