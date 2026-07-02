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
