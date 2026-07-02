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
