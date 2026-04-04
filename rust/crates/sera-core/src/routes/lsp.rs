//! LSP (Language Server Protocol) proxy endpoints.
//! Routes LSP requests to managed language server processes.

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use crate::error::AppError;
use crate::state::AppState;

#[derive(Deserialize)]
pub struct DefinitionRequest {
    pub file: String,
    pub line: u32,
    pub character: u32,
    pub language: Option<String>,
}

#[derive(Serialize)]
pub struct LocationResult {
    pub file: String,
    pub line: u32,
    pub character: u32,
    pub end_line: Option<u32>,
    pub end_character: Option<u32>,
}

/// POST /api/lsp/definition — get definition for symbol at position
pub async fn definition(
    State(_state): State<AppState>,
    Json(body): Json<DefinitionRequest>,
) -> Result<Json<Vec<LocationResult>>, AppError> {
    // LSP definition lookup requires a running language server
    // For now, return empty results — full implementation requires process management
    tracing::info!(
        file = %body.file, line = body.line, char = body.character,
        "LSP definition request"
    );

    // TODO: Route to running LSP server process for the file's language
    // This would use tokio::process::Command to communicate with the LSP
    Ok(Json(vec![]))
}

#[derive(Deserialize)]
pub struct ReferencesRequest {
    pub file: String,
    pub line: u32,
    pub character: u32,
    pub include_declaration: Option<bool>,
    pub language: Option<String>,
}

/// POST /api/lsp/references — find all references for symbol
pub async fn references(
    State(_state): State<AppState>,
    Json(body): Json<ReferencesRequest>,
) -> Result<Json<Vec<LocationResult>>, AppError> {
    tracing::info!(
        file = %body.file, line = body.line, char = body.character,
        "LSP references request"
    );

    // TODO: Route to running LSP server process
    Ok(Json(vec![]))
}

#[derive(Deserialize)]
pub struct SymbolsRequest {
    pub file: String,
    pub language: Option<String>,
}

#[derive(Serialize)]
pub struct DocumentSymbol {
    pub name: String,
    pub kind: String, // "function", "class", "variable", etc.
    pub range: SymbolRange,
    pub children: Vec<DocumentSymbol>,
}

#[derive(Serialize)]
pub struct SymbolRange {
    pub start_line: u32,
    pub start_character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// POST /api/lsp/symbols — get document symbols
pub async fn symbols(
    State(_state): State<AppState>,
    Json(body): Json<SymbolsRequest>,
) -> Result<Json<Vec<DocumentSymbol>>, AppError> {
    tracing::info!(file = %body.file, "LSP symbols request");

    // TODO: Route to running LSP server process
    Ok(Json(vec![]))
}
