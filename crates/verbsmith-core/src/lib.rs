//! Core types, parsing, workspace loading, and execution for Verbsmith.

mod engine;
mod error;
mod model;
mod parser;
mod workspace;

pub use engine::{ExecutionOptions, evaluate_assertions, execute, extract_captures};
pub use error::{Error, Result};
pub use model::{
    Assertion, AssertionResult, Capture, Environment, Header, HttpRequest, HttpResponse,
    OutputFormat, RunResult, SyncConfig, WorkspaceManifest,
};
pub use parser::{format_document, parse_document};
pub use workspace::{Workspace, discover_workspace};

/// Current on-disk workspace schema.
pub const WORKSPACE_SCHEMA_VERSION: u32 = 1;
