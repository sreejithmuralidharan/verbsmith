//! WebAssembly bridge used by browser clients such as API Studio.

use std::path::Path;

use wasm_bindgen::prelude::*;

/// Parse a `.http` document and return the canonical JSON model.
#[wasm_bindgen]
pub fn parse_http(path: &str, source: &str) -> Result<String, JsError> {
    let requests = verbsmith_core::parse_document(Path::new(path), source)
        .map_err(|error| JsError::new(&error.to_string()))?;
    serde_json::to_string(&requests).map_err(|error| JsError::new(&error.to_string()))
}

/// Format a `.http` document using the same parser as the native clients.
#[wasm_bindgen]
pub fn format_http(path: &str, source: &str) -> Result<String, JsError> {
    let requests = verbsmith_core::parse_document(Path::new(path), source)
        .map_err(|error| JsError::new(&error.to_string()))?;
    Ok(verbsmith_core::format_document(&requests))
}

/// Return the highest workspace schema understood by this build.
#[wasm_bindgen]
pub fn workspace_schema_version() -> u32 {
    verbsmith_core::WORKSPACE_SCHEMA_VERSION
}
