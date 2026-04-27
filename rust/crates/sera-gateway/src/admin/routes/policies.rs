//! Capability policy admin routes (sera-nrn9).
//!
//! `list` and `show` read directly from the configured policies directory;
//! `reload` re-runs `CapabilityRegistry::load_and_bind` and replaces the
//! gateway's `Arc<CapabilityRegistry>` atomically. `validate` is parse-only —
//! it never replaces the registry.

use std::path::Path;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;

use crate::admin::state::AdminAppState;
use crate::capability_enforcement::CapabilityRegistry;

/// GET /admin/policies — directory listing of `kind: CapabilityPolicy` files.
pub async fn list<S>(State(state): State<Arc<S>>) -> Json<serde_json::Value>
where
    S: AdminAppState,
{
    let dir = state.policies_dir();
    let names = read_policy_names(&dir);
    Json(serde_json::json!({
        "dir": dir.display().to_string(),
        "policies": names,
    }))
}

/// GET /admin/policies/{name} — raw YAML content of one policy file.
pub async fn show<S>(
    State(state): State<Arc<S>>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, StatusCode>
where
    S: AdminAppState,
{
    if !is_safe_name(&name) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let dir = state.policies_dir();
    let yaml = match find_and_read(&dir, &name) {
        Some(s) => s,
        None => return Err(StatusCode::NOT_FOUND),
    };

    let parsed: serde_yaml::Value = serde_yaml::from_str(&yaml)
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    let json: serde_json::Value =
        serde_json::to_value(parsed).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "name": name,
        "manifest": json,
    })))
}

/// POST /admin/policies/reload — re-load policies from disk and swap the
/// shared registry handle. Returns the names of the loaded policies.
pub async fn reload<S>(
    State(state): State<Arc<S>>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>
where
    S: AdminAppState,
{
    let dir = state.policies_dir();
    // We re-load with no agent bindings: the binding map is rebuilt from the
    // manifests at boot. Reload-without-restart of agents is a follow-up; for
    // L.3 the goal is just to re-read the policy bodies so capability changes
    // take effect on the next dispatch.
    let reg = CapabilityRegistry::load_and_bind::<_, String, String>(
        &dir,
        Vec::<(String, Option<String>)>::new(),
    )
    .map_err(|e| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "error": "policy_load_failed",
                "detail": e.to_string(),
            })),
        )
    })?;

    let mut names: Vec<String> = Vec::new();
    let _ = read_policy_names_into(&dir, &mut names);

    let new = Arc::new(reg);
    {
        let cell = state.capability_registry();
        let mut guard = cell.write().await;
        *guard = new;
    }

    tracing::info!(loaded = names.len(), "admin policy reload complete");
    Ok(Json(serde_json::json!({
        "loaded": names.len(),
        "policies": names,
    })))
}

/// POST /admin/policies/validate — try to parse each policy file and report
/// per-file errors. Never replaces the registry.
pub async fn validate<S>(
    State(state): State<Arc<S>>,
) -> Result<Json<serde_json::Value>, StatusCode>
where
    S: AdminAppState,
{
    let dir = state.policies_dir();
    if !dir.exists() {
        return Ok(Json(serde_json::json!({
            "dir": dir.display().to_string(),
            "ok": true,
            "results": [],
        })));
    }

    let mut results = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "yaml" && ext != "yml" {
            continue;
        }
        match std::fs::read_to_string(&path).and_then(|s| {
            serde_yaml::from_str::<serde_yaml::Value>(&s)
                .map_err(|e| std::io::Error::other(e.to_string()))
        }) {
            Ok(_) => results.push(serde_json::json!({"file": name, "ok": true})),
            Err(e) => results.push(serde_json::json!({
                "file": name,
                "ok": false,
                "error": e.to_string(),
            })),
        }
    }
    let all_ok = results
        .iter()
        .all(|r| r.get("ok").and_then(|v| v.as_bool()).unwrap_or(false));
    Ok(Json(serde_json::json!({
        "dir": dir.display().to_string(),
        "ok": all_ok,
        "results": results,
    })))
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
}

fn read_policy_names(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let _ = read_policy_names_into(dir, &mut out);
    out
}

fn read_policy_names_into(dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    out.clear();
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "yaml" && ext != "yml" {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            out.push(stem.to_string());
        }
    }
    out.sort();
    Ok(())
}

fn find_and_read(dir: &Path, name: &str) -> Option<String> {
    for ext in ["yaml", "yml"] {
        let path = dir.join(format!("{name}.{ext}"));
        if let Ok(s) = std::fs::read_to_string(&path) {
            return Some(s);
        }
    }
    None
}
