use rusqlite::Connection;
use tempfile::tempdir;
use trace_format::{TraceReader, TraceWriter};

#[test]
fn fresh_trace_is_v2_with_empty_identity_tables() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("v2.mlirtrace");
    let writer = TraceWriter::create(&path).unwrap();
    writer.finish().unwrap();

    let reader = TraceReader::open(&path).unwrap();
    assert_eq!(reader.meta().unwrap().get("format_version").unwrap(), "2");

    let connection = Connection::open(&path).unwrap();
    for table in ["op_index", "op_identity"] {
        let count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "table {table} should exist");
    }
}

#[test]
fn v1_trace_still_opens() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("v1.mlirtrace");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
             INSERT INTO meta(key, value) VALUES ('format_version', '1');",
        )
        .unwrap();
    drop(connection);

    let reader = TraceReader::open(&path).expect("v1 trace must open");
    assert_eq!(reader.meta().unwrap().get("format_version").unwrap(), "1");
}
