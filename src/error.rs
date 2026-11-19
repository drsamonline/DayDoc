use thiserror::Error;

/// Every failure path in DayDoc. Nothing is swallowed: a caller either gets a
/// note back or a reason.
#[derive(Debug, Error)]
pub enum DayDocError {
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("no vault at {0}")]
    NoVault(String),

    #[error("vault is read-only: {0}")]
    ReadOnly(String),

    #[error("refusing unsafe path outside the vault: {0}")]
    UnsafePath(String),

    #[error("broken frontmatter in {path}: {reason}")]
    BrokenFrontmatter { path: String, reason: String },

    #[error("decision {0} not found")]
    DecisionNotFound(String),

    #[error("invalid transition: {0}")]
    InvalidTransition(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("ledger error: {0}")]
    Ledger(String),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("yaml error: {0}")]
    Yaml(String),

    #[error("git error: {0}")]
    Git(String),
}

impl DayDocError {
    pub(crate) fn io(path: impl AsRef<std::path::Path>, source: std::io::Error) -> Self {
        DayDocError::Io {
            path: path.as_ref().display().to_string(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, DayDocError>;
