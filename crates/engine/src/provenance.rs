use std::collections::HashMap;
use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::{OpIdx, ParsedModule};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotSide {
    Before,
    After,
}

impl SnapshotSide {
    fn code(self) -> &'static str {
        match self {
            Self::Before => "b",
            Self::After => "a",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OpAnchor {
    pub function: String,
    pub pass_id: i64,
    pub side: SnapshotSide,
    pub function_ordinal: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct OpUid(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UidError(String);

impl fmt::Display for UidError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UidError {}

impl OpUid {
    pub fn from_anchor(anchor: &OpAnchor) -> Self {
        let function = URL_SAFE_NO_PAD.encode(anchor.function.as_bytes());
        Self(format!(
            "op1.{function}.{}.{}.{}",
            anchor.pass_id,
            anchor.side.code(),
            anchor.function_ordinal
        ))
    }

    pub fn parse(value: &str) -> Result<Self, UidError> {
        let uid = Self(value.to_string());
        uid.parse_anchor()?;
        Ok(uid)
    }

    pub fn parse_anchor(&self) -> Result<OpAnchor, UidError> {
        let fields: Vec<_> = self.0.split('.').collect();
        if fields.len() != 5 || fields[0] != "op1" {
            return Err(UidError("invalid op UID version or field count".into()));
        }
        let function_bytes = URL_SAFE_NO_PAD
            .decode(fields[1])
            .map_err(|_| UidError("invalid op UID function encoding".into()))?;
        let function = String::from_utf8(function_bytes)
            .map_err(|_| UidError("op UID function is not UTF-8".into()))?;
        let pass_id = fields[2]
            .parse()
            .map_err(|_| UidError("invalid op UID pass id".into()))?;
        let side = match fields[3] {
            "b" => SnapshotSide::Before,
            "a" => SnapshotSide::After,
            _ => return Err(UidError("invalid op UID side".into())),
        };
        let function_ordinal = fields[4]
            .parse()
            .map_err(|_| UidError("invalid op UID ordinal".into()))?;
        Ok(OpAnchor {
            function,
            pass_id,
            side,
            function_ordinal,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OccurrenceKey {
    pub stage_index: usize,
    pub side: SnapshotSide,
    pub op_idx: OpIdx,
}

#[derive(Debug, Clone)]
pub struct SnapshotOps {
    pub side: SnapshotSide,
    pub blob_id: Option<i64>,
    pub module: ParsedModule,
    pub function_ordinals: HashMap<OpIdx, usize>,
    pub tokens: HashMap<i64, OpIdx>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedIdentityKind {
    Inserted,
    Erased,
    Replaced,
    Modified,
}

#[derive(Debug, Clone)]
pub struct NormalizedIdentityEvent {
    pub kind: NormalizedIdentityKind,
    pub ptr_token: i64,
    pub new_token: Option<i64>,
    pub pattern: Option<String>,
    pub source: EvidenceSource,
    pub seq: i64,
}

#[derive(Debug, Clone)]
pub struct TimelineStage {
    pub pass_id: i64,
    pub pass_name: String,
    pub before: Option<SnapshotOps>,
    pub after: Option<SnapshotOps>,
    pub events: Vec<NormalizedIdentityEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Listener,
    Action,
    Fingerprint,
    SharedSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HistoryEvidence {
    pub seq: i64,
    pub pattern: Option<String>,
    pub source: EvidenceSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LinkConfidence {
    Exact,
    Inferred { score: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryChange {
    Inserted,
    Erased,
    Replaced,
    Modified,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct OpOccurrence {
    pub side: SnapshotSide,
    pub op_idx: OpIdx,
    pub name: String,
    pub line_start: usize,
    pub line_end: usize,
    pub attr_summary: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HistoryStep {
    pub pass_id: i64,
    pub pass_name: String,
    pub change: HistoryChange,
    pub before: Option<OpOccurrence>,
    pub after: Option<OpOccurrence>,
    pub evidence: Vec<HistoryEvidence>,
    pub confidence: LinkConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct OpHistory {
    pub uid: OpUid,
    pub first_name: String,
    pub last_name: String,
    pub steps: Vec<HistoryStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SelectableOp {
    pub uid: OpUid,
    pub op_idx: OpIdx,
    pub name: String,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedFunction {
    pub function: String,
    pub selectable: HashMap<OccurrenceKey, SelectableOp>,
    pub histories: HashMap<OpUid, OpHistory>,
}
