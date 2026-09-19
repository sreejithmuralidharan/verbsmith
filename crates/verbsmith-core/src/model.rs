use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

fn default_schema() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceManifest {
    #[serde(default = "default_schema")]
    pub schema: u32,
    pub name: String,
    #[serde(default = "default_request_glob")]
    pub request_glob: String,
    #[serde(default)]
    pub default_environment: Option<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
    #[serde(default)]
    pub sync: Option<SyncConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncConfig {
    pub endpoint: String,
    pub workspace_id: uuid::Uuid,
    #[serde(default = "default_sync_token_secret")]
    pub token_secret: String,
    #[serde(default = "default_sync_encryption_secret")]
    pub encryption_secret: String,
}

fn default_sync_token_secret() -> String {
    "sync-token".into()
}

fn default_sync_encryption_secret() -> String {
    "sync-key".into()
}

fn default_request_glob() -> String {
    "requests/**/*.http".into()
}

impl Default for WorkspaceManifest {
    fn default() -> Self {
        Self {
            schema: default_schema(),
            name: "verbsmith-workspace".into(),
            request_glob: default_request_glob(),
            default_environment: None,
            variables: BTreeMap::new(),
            sync: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Environment {
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    /// Values in this section are accepted for migration only. They are always redacted and the
    /// CLI warns users to move them into the encrypted vault.
    #[serde(default)]
    pub secrets: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Assertion {
    StatusEquals(u32),
    HeaderContains {
        name: String,
        value: String,
    },
    BodyContains(String),
    JsonEquals {
        path: String,
        expected: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Capture {
    JsonPath { name: String, path: String },
    Header { name: String, header: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpRequest {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<Header>,
    pub body: String,
    pub source: PathBuf,
    pub line: usize,
    pub timeout: Option<Duration>,
    pub tags: Vec<String>,
    pub depends_on: Vec<String>,
    pub assertions: Vec<Assertion>,
    pub captures: Vec<Capture>,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u32,
    pub headers: Vec<Header>,
    pub body: Vec<u8>,
    pub elapsed_ms: u128,
    pub effective_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionResult {
    pub description: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub request: HttpRequest,
    pub response: HttpResponse,
    pub assertions: Vec<AssertionResult>,
    pub captures: IndexMap<String, String>,
}

impl RunResult {
    pub fn passed(&self) -> bool {
        self.assertions.iter().all(|assertion| assertion.passed)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Pretty,
    Raw,
    Json,
    JsonLines,
    Junit,
    Sarif,
}
