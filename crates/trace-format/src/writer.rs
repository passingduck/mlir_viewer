use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use xxhash_rust::xxh3::xxh3_64;

use crate::error::Result;
use crate::identity::{IdentityEvent, OpIndexRow};
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
            .query_row(
                "SELECT id FROM ir_blob WHERE hash = ?1",
                params![&hash[..]],
                |r| r.get(0),
            )
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
                row.op_name.as_str(),
            ],
        )?;
        Ok(())
    }

    pub fn write_identity_event(&mut self, event: &IdentityEvent) -> Result<()> {
        self.conn.execute(
            "INSERT INTO op_identity
             (pass_id, kind, ptr_token, new_token, pattern, source, seq)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.pass.0,
                event.kind.as_str(),
                event.ptr_token,
                event.new_token,
                event.pattern.as_deref(),
                event.source.as_str(),
                event.seq,
            ],
        )?;
        Ok(())
    }

    /// Checkpoint and leave the file self-contained (no -wal/-shm sidecars),
    /// so a finished trace is a single copyable file.
    pub fn finish(self) -> Result<()> {
        self.conn.pragma_update(None, "journal_mode", "DELETE")?;
        Ok(())
    }
}
