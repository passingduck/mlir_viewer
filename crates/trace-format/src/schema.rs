/// Trace format major version written by this build.
pub const FORMAT_VERSION: &str = "2";

/// Versions this build can read. Version 1 predates identity capture.
pub const SUPPORTED_VERSIONS: &[&str] = &["1", "2"];

/// Full DDL for a v2 trace file. Matches design spec §5 plus M4 identity capture.
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
"#;
