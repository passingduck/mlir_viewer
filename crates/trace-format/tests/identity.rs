use rusqlite::Connection;
use tempfile::tempdir;
use trace_format::{
    IdentityEvent, IdentityKind, IdentitySource, OpIndexRow, PassId, PassRecord, Side, TraceError,
    TraceReader, TraceWriter,
};

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

#[test]
fn op_index_and_identity_round_trip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("rt.mlirtrace");
    let mut writer = TraceWriter::create(&path).unwrap();
    let before = writer.write_blob("%0 = arith.constant 1 : i32\n").unwrap();
    let after = writer.write_blob("return\n").unwrap();
    let pass = writer
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

    writer
        .write_op_index(&OpIndexRow {
            pass,
            side: Side::Before,
            ptr_token: 4096,
            byte_start: 0,
            byte_end: 27,
            op_name: "arith.constant".into(),
        })
        .unwrap();
    writer
        .write_identity_event(&IdentityEvent {
            pass,
            kind: IdentityKind::Erased,
            ptr_token: 4096,
            new_token: None,
            pattern: Some("DeadCodeElimination".into()),
            source: IdentitySource::Listener,
            seq: 0,
        })
        .unwrap();
    writer.finish().unwrap();

    let reader = TraceReader::open(&path).unwrap();
    let index = reader.op_index(pass).unwrap();
    assert_eq!(index.len(), 1);
    assert_eq!(index[0].op_name, "arith.constant");
    assert_eq!(index[0].side, Side::Before);
    assert_eq!(index[0].byte_end, 27);

    let events = reader.identity_events(pass).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, IdentityKind::Erased);
    assert_eq!(events[0].new_token, None);
    assert_eq!(events[0].pattern.as_deref(), Some("DeadCodeElimination"));
    assert_eq!(events[0].source, IdentitySource::Listener);
}

#[test]
fn identity_accessors_empty_on_v1() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("v1b.mlirtrace");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
             INSERT INTO meta(key, value) VALUES ('format_version', '1');",
        )
        .unwrap();
    drop(connection);

    let reader = TraceReader::open(&path).unwrap();
    assert!(reader.op_index(PassId(1)).unwrap().is_empty());
    assert!(reader.identity_events(PassId(1)).unwrap().is_empty());
}

#[test]
fn invalid_identity_encoding_is_rejected() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("invalid.mlirtrace");
    let mut writer = TraceWriter::create(&path).unwrap();
    let pass = writer
        .record_pass(&PassRecord {
            parent: None,
            seq: 0,
            name: "bad".into(),
            ir_before: None,
            ir_after: None,
            start_ns: 0,
            end_ns: 1,
            ir_changed: false,
        })
        .unwrap();
    writer.finish().unwrap();

    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO op_identity
             (pass_id, kind, ptr_token, new_token, pattern, source, seq)
             VALUES (?1, 'not-a-kind', 1, NULL, NULL, 'listener', 0)",
            [pass.0],
        )
        .unwrap();
    drop(connection);

    let reader = TraceReader::open(&path).unwrap();
    assert!(matches!(
        reader.identity_events(pass),
        Err(TraceError::Corrupt(message)) if message.contains("not-a-kind")
    ));
}
