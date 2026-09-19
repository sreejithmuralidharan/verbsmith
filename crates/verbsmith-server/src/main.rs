use std::{
    env,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, put},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    database: Arc<Mutex<Connection>>,
    token: Arc<str>,
}

#[derive(Debug, Deserialize)]
struct PushRevision {
    base_revision: Option<String>,
    ciphertext: String,
}

#[derive(Debug, Serialize)]
struct Revision {
    id: String,
    workspace_id: String,
    parent_id: Option<String>,
    sha256: String,
    ciphertext: String,
    created_at: String,
}

#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "verbsmith_server=info,tower_http=info".into()),
        )
        .init();

    let address = env::var("VERBSMITH_SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8787".into());
    let token = env::var("VERBSMITH_SERVER_TOKEN")
        .context("VERBSMITH_SERVER_TOKEN must be set; refusing to start without authentication")?;
    if token.len() < 24 {
        anyhow::bail!("VERBSMITH_SERVER_TOKEN must contain at least 24 characters");
    }
    let database_path = env::var_os("VERBSMITH_SERVER_DATABASE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("verbsmith-server.sqlite3"));
    let connection = Connection::open(database_path)?;
    migrate(&connection)?;
    let state = AppState {
        database: Arc::new(Mutex::new(connection)),
        token: token.into(),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/workspaces/{workspace_id}/head", get(get_head))
        .route(
            "/api/v1/workspaces/{workspace_id}/revisions",
            put(push_revision),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address, "Verbsmith sync server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn get_head(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Revision>, ApiError> {
    authenticate(&state, &headers)?;
    validate_workspace_id(&workspace_id)?;
    let database = state
        .database
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let revision = database
        .query_row(
            "SELECT id, workspace_id, parent_id, sha256, ciphertext, created_at FROM revisions WHERE workspace_id = ?1 ORDER BY sequence DESC LIMIT 1",
            [&workspace_id],
            |row| Ok(Revision {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                parent_id: row.get(2)?,
                sha256: row.get(3)?,
                ciphertext: row.get(4)?,
                created_at: row.get(5)?,
            }),
        )
        .optional()
        .map_err(ApiError::database)?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "workspace has no revisions"))?;
    Ok(Json(revision))
}

async fn push_revision(
    State(state): State<AppState>,
    Path(workspace_id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<PushRevision>,
) -> Result<(StatusCode, Json<Revision>), ApiError> {
    authenticate(&state, &headers)?;
    validate_workspace_id(&workspace_id)?;
    if payload.ciphertext.len() > 20 * 1024 * 1024 {
        return Err(ApiError::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "encrypted revision exceeds 20 MiB",
        ));
    }
    let mut database = state
        .database
        .lock()
        .map_err(|_| ApiError::internal("database lock poisoned"))?;
    let transaction = database.transaction().map_err(ApiError::database)?;
    let head = transaction
        .query_row(
            "SELECT id FROM revisions WHERE workspace_id = ?1 ORDER BY sequence DESC LIMIT 1",
            [&workspace_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(ApiError::database)?;
    if head != payload.base_revision {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "base revision is not the current head",
        ));
    }

    let id = Uuid::new_v4().to_string();
    let sha256 = format!("{:x}", Sha256::digest(payload.ciphertext.as_bytes()));
    let created_at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| ApiError::internal("failed to format revision timestamp"))?;
    transaction
        .execute(
            "INSERT INTO revisions (id, workspace_id, parent_id, sha256, ciphertext, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, workspace_id, payload.base_revision, sha256, payload.ciphertext, created_at],
        )
        .map_err(ApiError::database)?;
    transaction.commit().map_err(ApiError::database)?;
    let revision = Revision {
        id,
        workspace_id,
        parent_id: payload.base_revision,
        sha256,
        ciphertext: payload.ciphertext,
        created_at,
    };
    Ok((StatusCode::CREATED, Json(revision)))
}

fn migrate(database: &Connection) -> Result<()> {
    database.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS revisions (
           sequence INTEGER PRIMARY KEY AUTOINCREMENT,
           id TEXT NOT NULL UNIQUE,
           workspace_id TEXT NOT NULL,
           parent_id TEXT,
           sha256 TEXT NOT NULL,
           ciphertext TEXT NOT NULL,
           created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
         );
         CREATE INDEX IF NOT EXISTS revisions_workspace_sequence
           ON revisions(workspace_id, sequence DESC);",
    )?;
    Ok(())
}

fn authenticate(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let supplied = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let authenticated = supplied.is_some_and(|value| {
        value.len() == state.token.len()
            && bool::from(value.as_bytes().ct_eq(state.token.as_bytes()))
    });
    if authenticated {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid bearer token",
        ))
    }
}

fn validate_workspace_id(id: &str) -> Result<(), ApiError> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "workspace_id must be a UUID"))
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn database(error: rusqlite::Error) -> Self {
        tracing::error!(%error, "database operation failed");
        Self::internal("database operation failed")
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({"error": self.message})),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        let database = Connection::open_in_memory().unwrap();
        migrate(&database).unwrap();
        AppState {
            database: Arc::new(Mutex::new(database)),
            token: Arc::from("a-secure-test-token-with-24-chars"),
        }
    }

    #[test]
    fn initializes_revision_schema() {
        let state = state();
        let database = state.database.lock().unwrap();
        let tables: i64 = database
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='revisions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tables, 1);
    }

    #[test]
    fn rejects_missing_auth_and_invalid_workspace_ids() {
        assert!(authenticate(&state(), &HeaderMap::new()).is_err());
        assert!(validate_workspace_id("not-a-uuid").is_err());
        assert!(validate_workspace_id(&Uuid::new_v4().to_string()).is_ok());
    }
}
