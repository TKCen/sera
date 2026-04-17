//! `sera auth` — login, whoami, logout.
//!
//! No OIDC device-flow is wired in the gateway today, so login prompts for an
//! API key interactively (via `rpassword`) then validates it against
//! `GET /api/auth/me`.  The token is stored via the platform-best
//! [`crate::token_store::TokenStore`] (OS keyring → file fallback).
//!
//! Each command struct accepts an optional injected [`TokenStore`] so tests
//! can pass a [`MockTokenStore`] without touching the real keychain.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use sera_commands::{
    Command, CommandArgSchema, CommandArgs, CommandCategory, CommandContext, CommandDescription,
    CommandError, CommandResult,
};

use crate::http::build_client_with_token;
use crate::token_store::{best_available_store, TokenStore};

// ---------------------------------------------------------------------------
// LoginCommand
// ---------------------------------------------------------------------------

/// `sera auth login` — prompt for API key, validate, store.
pub struct LoginCommand {
    store: Arc<dyn TokenStore>,
}

impl LoginCommand {
    /// Production constructor — uses the platform-best store.
    pub fn new() -> Self {
        Self {
            store: Arc::from(best_available_store()),
        }
    }

    /// Test constructor — inject a custom store.
    pub fn with_store(store: Arc<dyn TokenStore>) -> Self {
        Self { store }
    }
}

impl Default for LoginCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for LoginCommand {
    fn name(&self) -> &str {
        "auth:login"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Authenticate against the SERA gateway and store a token".into(),
            help: "Prompts for an API key, validates it against GET /api/auth/me, then stores \
                   the token in the OS keyring (falling back to ~/.sera/token on platforms \
                   without a keychain daemon)."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("login")
                .about("Authenticate against the SERA gateway")
                .arg(
                    clap::Arg::new("endpoint")
                        .long("endpoint")
                        .short('e')
                        .help("Gateway base URL (overrides config)")
                        .value_name("URL"),
                )
                .arg(
                    clap::Arg::new("token")
                        .long("token")
                        .help("Supply token non-interactively (for scripts/tests)")
                        .value_name("TOKEN"),
                ),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let endpoint = args
            .get("endpoint")
            .unwrap_or("http://localhost:8080")
            .trim_end_matches('/')
            .to_owned();

        // Obtain the token — `--token` flag bypasses interactive prompt (useful
        // in tests and non-TTY environments).
        let token = if let Some(t) = args.get("token") {
            t.to_owned()
        } else {
            rpassword::prompt_password("SERA API key: ")
                .map_err(|e| CommandError::Execution(format!("failed to read password: {e}")))?
        };

        let token = token.trim().to_owned();
        if token.is_empty() {
            return Err(CommandError::InvalidArgs("token must not be empty".into()));
        }

        // Validate against /api/auth/me
        let client = build_client_with_token(&token)
            .map_err(|e| CommandError::Execution(e.to_string()))?;
        let url = format!("{endpoint}/api/auth/me");
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CommandError::Execution(
                "authentication failed: invalid API key".into(),
            ));
        }
        if !response.status().is_success() {
            return Err(CommandError::Execution(format!(
                "gateway returned HTTP {}",
                response.status()
            )));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| CommandError::Execution(format!("failed to parse response: {e}")))?;

        // Store the token
        self.store
            .save(&token)
            .map_err(|e| CommandError::Execution(format!("failed to store token: {e}")))?;

        // Support both the full gateway shape (`sub`) and the autonomous shape
        // (`id`, `principal_id`). Use the first non-empty value found.
        let sub = body.get("sub")
            .or_else(|| body.get("id"))
            .or_else(|| body.get("principal_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        println!("Logged in as: {sub}");

        Ok(CommandResult::ok(json!({
            "sub": sub,
            "endpoint": endpoint,
        })))
    }
}

// ---------------------------------------------------------------------------
// WhoamiCommand
// ---------------------------------------------------------------------------

/// `sera auth whoami` — show the authenticated principal.
pub struct WhoamiCommand {
    store: Arc<dyn TokenStore>,
}

impl WhoamiCommand {
    /// Production constructor — uses the platform-best store.
    pub fn new() -> Self {
        Self {
            store: Arc::from(best_available_store()),
        }
    }

    /// Test constructor — inject a custom store.
    pub fn with_store(store: Arc<dyn TokenStore>) -> Self {
        Self { store }
    }
}

impl Default for WhoamiCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for WhoamiCommand {
    fn name(&self) -> &str {
        "auth:whoami"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Print the currently authenticated principal".into(),
            help: "Loads the stored token and calls GET /api/auth/me to retrieve the \
                   principal and roles."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(
            clap::Command::new("whoami")
                .about("Print the currently authenticated principal")
                .arg(
                    clap::Arg::new("endpoint")
                        .long("endpoint")
                        .short('e')
                        .help("Gateway base URL (overrides config)")
                        .value_name("URL"),
                ),
        )
    }

    async fn execute(
        &self,
        args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        let endpoint = args
            .get("endpoint")
            .unwrap_or("http://localhost:8080")
            .trim_end_matches('/')
            .to_owned();

        let token = self
            .store
            .load()
            .map_err(|e| CommandError::Execution(format!("failed to load token: {e}")))?
            .ok_or_else(|| {
                CommandError::Execution("not logged in — run `sera auth login` first".into())
            })?;

        let client = build_client_with_token(&token)
            .map_err(|e| CommandError::Execution(e.to_string()))?;
        let url = format!("{endpoint}/api/auth/me");
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| CommandError::Execution(format!("request failed: {e}")))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(CommandError::Execution(
                "token rejected — run `sera auth login` again".into(),
            ));
        }
        if !response.status().is_success() {
            return Err(CommandError::Execution(format!(
                "gateway returned HTTP {}",
                response.status()
            )));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| CommandError::Execution(format!("failed to parse response: {e}")))?;

        // Support both the full gateway shape (`sub`) and the autonomous shape
        // (`id`, `principal_id`). Use the first non-empty value found.
        let sub = body.get("sub")
            .or_else(|| body.get("id"))
            .or_else(|| body.get("principal_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let roles = body
            .get("roles")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| r.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        println!("sub:   {sub}");
        if !roles.is_empty() {
            println!("roles: {roles}");
        }

        Ok(CommandResult::ok(body))
    }
}

// ---------------------------------------------------------------------------
// LogoutCommand
// ---------------------------------------------------------------------------

/// `sera auth logout` — remove the stored token.
pub struct LogoutCommand {
    store: Arc<dyn TokenStore>,
}

impl LogoutCommand {
    /// Production constructor — uses the platform-best store.
    pub fn new() -> Self {
        Self {
            store: Arc::from(best_available_store()),
        }
    }

    /// Test constructor — inject a custom store.
    pub fn with_store(store: Arc<dyn TokenStore>) -> Self {
        Self { store }
    }
}

impl Default for LogoutCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Command for LogoutCommand {
    fn name(&self) -> &str {
        "auth:logout"
    }

    fn describe(&self) -> CommandDescription {
        CommandDescription {
            summary: "Remove the stored SERA token".into(),
            help: "Deletes the bearer token from the OS keyring and the fallback token file."
                .into(),
            category: CommandCategory::System,
        }
    }

    fn argument_schema(&self) -> CommandArgSchema {
        CommandArgSchema(clap::Command::new("logout").about("Remove the stored SERA token"))
    }

    async fn execute(
        &self,
        _args: CommandArgs,
        _ctx: &CommandContext,
    ) -> Result<CommandResult, CommandError> {
        self.store
            .clear()
            .map_err(|e| CommandError::Execution(format!("failed to clear token: {e}")))?;
        println!("Logged out.");
        Ok(CommandResult::success())
    }
}
