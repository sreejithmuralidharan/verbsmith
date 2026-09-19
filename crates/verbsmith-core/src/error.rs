use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not find verbsmith.toml from {0}")]
    WorkspaceNotFound(PathBuf),
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("workspace schema {found} is newer than supported schema {supported}")]
    UnsupportedSchema { found: u32, supported: u32 },
    #[error("request `{0}` was not found")]
    RequestNotFound(String),
    #[error("unresolved variable `{0}`")]
    UnresolvedVariable(String),
    #[error("invalid URL `{0}`")]
    InvalidUrl(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("response exceeded the configured {limit} byte limit")]
    ResponseTooLarge { limit: usize },
    #[error("dependency cycle contains request `{0}`")]
    DependencyCycle(String),
    #[error("request `{request}` depends on unknown request `{dependency}`")]
    UnknownDependency { request: String, dependency: String },
}

pub type Result<T> = std::result::Result<T, Error>;
