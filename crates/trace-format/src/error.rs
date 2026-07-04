#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported trace format version {found} (reader supports {supported})")]
    VersionMismatch {
        found: String,
        supported: &'static str,
    },
    #[error("corrupt trace: {0}")]
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, TraceError>;
