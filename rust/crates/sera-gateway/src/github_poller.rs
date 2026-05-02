//! Production GitHub poller for the GhPr / GhRun workflow gates (sera-ai4w).
//!
//! Background tokio task that periodically queries the GitHub REST API for the
//! IDs currently registered in the [`GhPrStateStore`] and [`GhRunStateStore`]
//! and pushes resolved states back into those stores so the scheduler's
//! per-tick snapshot lookup wakes the right workflow tasks.
//!
//! Design notes:
//! - **No octocrab dependency.** The polling surface is two endpoints; pulling
//!   in a 1MB+ generated client crate just to call them would inflate the
//!   binary noticeably. We use the workspace's existing `reqwest` instead.
//! - **PAT auth only.** `SERA_GH_TOKEN` is read once at boot and threaded
//!   through every request. App-installation flows are out of scope and
//!   tracked as a follow-up.
//! - **Pull-only.** The poller never inserts new IDs into the stores — it only
//!   refreshes states for IDs the gates have already registered (via the
//!   `/api/workflow/tasks` route or test code). Unknown IDs stay `Unknown`.
//! - **Best-effort.** A failed HTTP call is logged and the entry is left
//!   untouched so the next tick retries. We never poison a store entry on a
//!   transient network blip.
//!
//! Gated behind the `gh-api` cargo feature so deployments that do not need the
//! GitHub gates can opt out and skip the reqwest TLS handshake setup at boot.

#![cfg(feature = "gh-api")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Deserialize;

use sera_workflow::task::{GhPrId, GhPrState, GhRunId, GhRunStatus};

use crate::workflow_store::{GhPrStateStore, GhRunStateStore};

/// Default poll interval. Long enough to stay well under the 5000-req/hour
/// authenticated rate limit even with hundreds of pending gates, short enough
/// that workflow tasks resume within a minute of the underlying PR / run
/// reaching a terminal state.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Configuration for the production GitHub poller.
#[derive(Clone)]
pub struct GitHubPollerConfig {
    pub token: String,
    pub interval: Duration,
    pub base_url: String,
    pub user_agent: String,
}

impl GitHubPollerConfig {
    /// Build a config from `SERA_GH_TOKEN` + `SERA_GH_POLL_SECS` env vars.
    /// Returns `None` when no token is set — callers fall back to the no-op
    /// path.
    pub fn from_env() -> Option<Self> {
        let token = std::env::var("SERA_GH_TOKEN").ok().filter(|s| !s.is_empty())?;
        let interval = std::env::var("SERA_GH_POLL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|n| *n > 0)
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_POLL_INTERVAL);
        Some(Self {
            token,
            interval,
            base_url: "https://api.github.com".to_string(),
            user_agent: "sera-gateway/0.1".to_string(),
        })
    }
}

struct PrCoord<'a> {
    owner: &'a str,
    repo: &'a str,
    number: u64,
}

struct RunCoord<'a> {
    owner: &'a str,
    repo: &'a str,
    run_id: u64,
}

fn parse_pr_id(raw: &str) -> Option<PrCoord<'_>> {
    let (repo_path, num) = raw.split_once('#')?;
    let (owner, repo) = repo_path.split_once('/')?;
    let number: u64 = num.parse().ok()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(PrCoord { owner, repo, number })
}

fn parse_run_id(raw: &str) -> Option<RunCoord<'_>> {
    let (repo_path, run) = raw.split_once(':')?;
    let (owner, repo) = repo_path.split_once('/')?;
    let run_id: u64 = run.parse().ok()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(RunCoord { owner, repo, run_id })
}

#[derive(Debug, Deserialize)]
struct PrResponse {
    #[serde(default)]
    state: String,
    #[serde(default)]
    merged: bool,
    #[serde(default)]
    draft: bool,
}

fn pr_response_to_state(r: &PrResponse) -> GhPrState {
    if r.merged {
        return GhPrState::Merged;
    }
    match r.state.as_str() {
        "open" if r.draft => GhPrState::Draft,
        "open" => GhPrState::Open,
        "closed" => GhPrState::Closed,
        _ => GhPrState::Unknown,
    }
}

#[derive(Debug, Deserialize)]
struct RunResponse {
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
}

fn run_response_to_status(r: &RunResponse) -> GhRunStatus {
    match r.status.as_str() {
        "queued" | "waiting" | "pending" | "requested" => return GhRunStatus::Queued,
        "in_progress" => return GhRunStatus::InProgress,
        "completed" => {}
        _ => return GhRunStatus::Unknown,
    }
    match r.conclusion.as_deref() {
        Some("success") => GhRunStatus::Completed,
        Some("failure") | Some("timed_out") | Some("startup_failure") => GhRunStatus::Failed,
        Some("cancelled") => GhRunStatus::Cancelled,
        Some("skipped") => GhRunStatus::Skipped,
        Some("neutral") | Some("action_required") => GhRunStatus::Neutral,
        _ => GhRunStatus::Unknown,
    }
}

/// Spawn the production GitHub poller as a background tokio task.
pub fn spawn_poller(
    config: GitHubPollerConfig,
    gh_pr_store: Option<Arc<dyn GhPrStateStore>>,
    gh_run_store: Option<Arc<dyn GhRunStateStore>>,
    shutting_down: Arc<AtomicBool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = match build_client(&config) {
            Ok(c) => c,
            Err(err) => {
                tracing::error!(
                    target: "sera_gateway::github_poller",
                    error = %err,
                    "failed to build GitHub poller HTTP client; poller will not run"
                );
                return;
            }
        };
        let mut ticker = tokio::time::interval(config.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await;
        loop {
            if shutting_down.load(Ordering::SeqCst) {
                tracing::info!(
                    target: "sera_gateway::github_poller",
                    "GitHub poller exiting (shutdown signalled)"
                );
                break;
            }
            ticker.tick().await;
            if shutting_down.load(Ordering::SeqCst) {
                break;
            }
            poll_once(&client, &config, &gh_pr_store, &gh_run_store).await;
        }
    })
}

fn build_client(config: &GitHubPollerConfig) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .user_agent(&config.user_agent)
        .timeout(Duration::from_secs(15))
        .build()
}

/// Run a single poll pass. Public so tests can drive deterministically.
pub async fn poll_once(
    client: &reqwest::Client,
    config: &GitHubPollerConfig,
    gh_pr_store: &Option<Arc<dyn GhPrStateStore>>,
    gh_run_store: &Option<Arc<dyn GhRunStateStore>>,
) {
    if let Some(store) = gh_pr_store {
        let snapshot = store.snapshot().await;
        for (id, _state) in snapshot {
            let Some(coord) = parse_pr_id(&id) else {
                tracing::warn!(
                    target: "sera_gateway::github_poller",
                    pr_id = %id,
                    "skipping PR id — expected `owner/repo#number` form"
                );
                continue;
            };
            match fetch_pr(client, config, &coord).await {
                Ok(state) => store.upsert(GhPrId::new(id.clone()), state).await,
                Err(err) => tracing::warn!(
                    target: "sera_gateway::github_poller",
                    pr_id = %id,
                    error = %err,
                    "GitHub PR poll failed; will retry next interval"
                ),
            }
        }
    }

    if let Some(store) = gh_run_store {
        let snapshot = store.snapshot().await;
        for (id, _status) in snapshot {
            let Some(coord) = parse_run_id(&id) else {
                tracing::warn!(
                    target: "sera_gateway::github_poller",
                    run_id = %id,
                    "skipping run id — expected `owner/repo:run_id` form"
                );
                continue;
            };
            match fetch_run(client, config, &coord).await {
                Ok(status) => store.upsert(GhRunId::new(id.clone()), status).await,
                Err(err) => tracing::warn!(
                    target: "sera_gateway::github_poller",
                    run_id = %id,
                    error = %err,
                    "GitHub run poll failed; will retry next interval"
                ),
            }
        }
    }
}

async fn fetch_pr(
    client: &reqwest::Client,
    config: &GitHubPollerConfig,
    coord: &PrCoord<'_>,
) -> Result<GhPrState, reqwest::Error> {
    let url = format!(
        "{}/repos/{}/{}/pulls/{}",
        config.base_url.trim_end_matches('/'),
        coord.owner,
        coord.repo,
        coord.number
    );
    let resp = client
        .get(&url)
        .bearer_auth(&config.token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Ok(GhPrState::Unknown);
    }
    let body: PrResponse = resp.json().await?;
    Ok(pr_response_to_state(&body))
}

async fn fetch_run(
    client: &reqwest::Client,
    config: &GitHubPollerConfig,
    coord: &RunCoord<'_>,
) -> Result<GhRunStatus, reqwest::Error> {
    let url = format!(
        "{}/repos/{}/{}/actions/runs/{}",
        config.base_url.trim_end_matches('/'),
        coord.owner,
        coord.repo,
        coord.run_id
    );
    let resp = client
        .get(&url)
        .bearer_auth(&config.token)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        return Ok(GhRunStatus::Unknown);
    }
    let body: RunResponse = resp.json().await?;
    Ok(run_response_to_status(&body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pr_id_accepts_owner_repo_number() {
        let c = parse_pr_id("acme/widgets#42").unwrap();
        assert_eq!(c.owner, "acme");
        assert_eq!(c.repo, "widgets");
        assert_eq!(c.number, 42);
    }

    #[test]
    fn parse_pr_id_rejects_malformed() {
        assert!(parse_pr_id("nope").is_none());
        assert!(parse_pr_id("acme#42").is_none());
        assert!(parse_pr_id("acme/widgets#abc").is_none());
        assert!(parse_pr_id("/widgets#42").is_none());
    }

    #[test]
    fn parse_run_id_accepts_owner_repo_run() {
        let c = parse_run_id("acme/widgets:99").unwrap();
        assert_eq!(c.owner, "acme");
        assert_eq!(c.repo, "widgets");
        assert_eq!(c.run_id, 99);
    }

    #[test]
    fn pr_state_mapping_handles_open_merged_closed_draft() {
        let merged = PrResponse { state: "closed".into(), merged: true, draft: false };
        assert!(matches!(pr_response_to_state(&merged), GhPrState::Merged));
        let closed = PrResponse { state: "closed".into(), merged: false, draft: false };
        assert!(matches!(pr_response_to_state(&closed), GhPrState::Closed));
        let open = PrResponse { state: "open".into(), merged: false, draft: false };
        assert!(matches!(pr_response_to_state(&open), GhPrState::Open));
        let draft = PrResponse { state: "open".into(), merged: false, draft: true };
        assert!(matches!(pr_response_to_state(&draft), GhPrState::Draft));
    }

    #[test]
    fn run_status_mapping_handles_known_combinations() {
        let queued = RunResponse { status: "queued".into(), conclusion: None };
        assert!(matches!(run_response_to_status(&queued), GhRunStatus::Queued));
        let success = RunResponse {
            status: "completed".into(),
            conclusion: Some("success".into()),
        };
        assert!(matches!(run_response_to_status(&success), GhRunStatus::Completed));
        let failure = RunResponse {
            status: "completed".into(),
            conclusion: Some("failure".into()),
        };
        assert!(matches!(run_response_to_status(&failure), GhRunStatus::Failed));
    }
}
