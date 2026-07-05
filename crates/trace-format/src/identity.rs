//! Shared value types and canonical SQL encodings for schema-v2 identity data.

use crate::writer::PassId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
}

impl Side {
    pub fn to_i64(self) -> i64 {
        match self {
            Self::Before => 0,
            Self::After => 1,
        }
    }

    pub fn from_i64(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::Before),
            1 => Some(Self::After),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityKind {
    Inserted,
    Erased,
    Replaced,
    Modified,
}

impl IdentityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inserted => "inserted",
            Self::Erased => "erased",
            Self::Replaced => "replaced",
            Self::Modified => "modified",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "inserted" => Some(Self::Inserted),
            "erased" => Some(Self::Erased),
            "replaced" => Some(Self::Replaced),
            "modified" => Some(Self::Modified),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySource {
    Listener,
    Action,
}

impl IdentitySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Listener => "listener",
            Self::Action => "action",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "listener" => Some(Self::Listener),
            "action" => Some(Self::Action),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpIndexRow {
    pub pass: PassId,
    pub side: Side,
    pub ptr_token: i64,
    pub byte_start: i64,
    pub byte_end: i64,
    pub op_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityEvent {
    pub pass: PassId,
    pub kind: IdentityKind,
    pub ptr_token: i64,
    pub new_token: Option<i64>,
    pub pattern: Option<String>,
    pub source: IdentitySource,
    pub seq: i64,
}
