//! Sandbox container management endpoints.
#![allow(dead_code, unused_imports)]

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::AppError;
use crate::state::AppState;

/// Policy violation error for forbidden actions
#[derive(Debug, Serialize)]
pub struct PolicyViolation {
    pub error: String,
    pub violation: String,
}

#[derive(Deserialize)]
pub struct SpawnRequest {
    pub image: String,
    #[serde(default = "default_network")]
    pub network: String,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    pub name: Option<String>,
}

fn default_network() -> String {
    "sera_net".to_string()
}

#[derive(Serialize)]
pub struct SpawnResponse {
    pub container_id: String,
    pub name: String,
    pub status: String,
}

/// POST /api/sandbox/spawn — create and start a container
pub async fn spawn(
    State(state): State<AppState>,
    Json(body): Json<SpawnRequest>,
) -> Result<(StatusCode, Json<SpawnResponse>), AppError> {
    let container_name = body
        .name
        .unwrap_or_else(|| format!("sera-sandbox-{}", &uuid::Uuid::new_v4().to_string()[..8]));

    let config = sera_tools::sandbox::SandboxConfig {
        image: Some(body.image.clone()),
        env: body.env,
        labels: body.labels,
        ..Default::default()
    };

    let handle = state
        .sandbox
        .create(&config)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Sandbox spawn failed: {e}")))?;

    let container_id = handle.0;

    Ok((
        StatusCode::CREATED,
        Json(SpawnResponse {
            container_id,
            name: container_name,
            status: "running".to_string(),
        }),
    ))
}

#[derive(Deserialize)]
pub struct ExecRequest {
    pub container_id: String,
    pub command: Vec<String>,
    #[serde(default)]
    pub working_dir: Option<String>,
}

#[derive(Serialize)]
pub struct ExecResponse {
    pub exit_code: i64,
    pub stdout: String,
    pub stderr: String,
}

/// POST /api/sandbox/exec — execute command in running container
pub async fn exec(
    State(state): State<AppState>,
    Json(body): Json<ExecRequest>,
) -> Result<Json<ExecResponse>, AppError> {
    let cmd = body.command.join(" ");
    let handle = sera_tools::sandbox::SandboxHandle(body.container_id.clone());
    let output = state
        .sandbox
        .execute(&handle, &cmd, &HashMap::new())
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("Sandbox exec failed: {e}")))?;

    Ok(Json(ExecResponse {
        exit_code: output.exit_code as i64,
        stdout: output.stdout,
        stderr: output.stderr,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_request_defaults() {
        let json = serde_json::json!({
            "image": "alpine:latest"
        });
        let req: SpawnRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.image, "alpine:latest");
        assert_eq!(req.network, "sera_net");
        assert!(req.env.is_empty());
        assert!(req.labels.is_empty());
        assert!(req.name.is_none());
    }

    #[test]
    fn spawn_request_with_custom_network() {
        let json = serde_json::json!({
            "image": "alpine:latest",
            "network": "custom_net"
        });
        let req: SpawnRequest = serde_json::from_value(json).unwrap();
        assert_eq!(req.network, "custom_net");
    }
}
