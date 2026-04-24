//! SERA end-to-end test harness.
//!
//! This crate ships the `InProcessGateway` helper — it is *in-process* in the
//! sense that the test-side controller lives inside the `cargo test` process,
//! but the gateway itself runs as a spawned `sera-gateway` child.  We chose
//! spawn-over-embed because the gateway binary (`sera-gateway/src/bin/sera.rs`)
//! wires its own tracing, SQLite, Discord connector, runtime-harness fan-out,
//! and SIGTERM drain inside `run_start()` without exposing a reusable boot
//! function.  Replicating that boot sequence inside a test would be both
//! invasive *and* fragile (it drifts every time `run_start` changes), so the
//! harness treats the gateway as a black box and talks to it over HTTP /
//! sqlite — exactly the surface a real operator uses.
//!
//! The harness is deliberately small: it gives tests a known-good temp
//! working directory with a `sera.yaml` + `secrets/` + `sera.db`, tells the
//! gateway to bind an ephemeral port, and hands back a base URL + db path.
//! Everything else (auth flow, agent lookup, turn assertions) lives in the
//! integration test itself so each scenario can be read as a single story.
//!
//! ## Profiles
//!
//! The crate exposes three Cargo features:
//!
//! - `integration` — turns on the real end-to-end test in
//!   `tests/local_profile_turn.rs`.  Default `cargo test` leaves this off so
//!   the crate compiles in a bare workspace check without spinning any
//!   servers.
//! - `postgres` — enables the enterprise profile: `DATABASE_URL` must point
//!   at a Postgres instance with pgvector; the test additionally asserts a
//!   Tier-2 semantic recall hit landed in the turn context.  Implies
//!   `integration`.
//! - `centrifugo` — subscribes to `agent:{id}:thoughts` and asserts at least
//!   one `type == "thought_stream"` event is emitted during the turn.
//!   Skips cleanly (does not fail) if `CENTRIFUGO_URL` is unset.  Implies
//!   `integration`.
//!
//! ## Environment variables
//!
//! - `SERA_E2E_LLM_BASE_URL` — base URL of the LLM provider to use during
//!   the test run (e.g. `http://localhost:1234/v1` for LM Studio).  When
//!   unset, the harness falls back to the built-in wiremock mock.
//! - `SERA_E2E_MODEL` — model identifier written into the generated
//!   `sera.yaml` for both `default_model` and the agent's `model` field.
//!   When unset or empty, defaults to `"e2e-mock"`, which is accepted by the
//!   wiremock fallback.  Set this to your real model name (e.g.
//!   `"lmstudio-community/meta-llama-3-8b"`) when pointing
//!   `SERA_E2E_LLM_BASE_URL` at a real provider.
//! - `SERA_E2E_LOG` — `RUST_LOG` value forwarded to the spawned gateway
//!   child.  Defaults to `"warn"`.
//!
//! ## Skip contract
//!
//! The real integration test exercises a live LLM by default.  When
//! `SERA_E2E_LLM_BASE_URL` is unset *and* we cannot spawn a local
//! `wiremock`-backed mock LLM, the test emits a single skip line to stderr
//! and returns `Ok(())` rather than panicking.  This keeps CI green in
//! environments without outbound LLM access.

use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

pub mod binaries;
pub mod mock_llm;

/// The gateway binary target name as declared in `sera-gateway/Cargo.toml`.
///
/// Tests should use `env!("CARGO_BIN_EXE_sera-gateway")` directly because the
/// `CARGO_BIN_EXE_*` variable is only set inside the test crate itself —
/// library crates cannot reference it.  The constant is exported here to
/// document the expected name and keep the two in sync manually.
pub const GATEWAY_BIN_NAME: &str = "sera-gateway";

/// Name of the `sera-runtime` child process the gateway spawns per-agent.
/// The test harness threads the cargo-built path for this binary into the
/// gateway's environment via `SERA_RUNTIME_BIN` so it does not depend on the
/// current working directory having a pre-built runtime next to the gateway.
pub const RUNTIME_BIN_NAME: &str = "sera-runtime";

/// How long to wait for the gateway's `/api/health` to return `200` before
/// giving up and panicking.  A bare boot on a warm target directory takes
/// well under a second; ten seconds is generous but still safely below the
/// 90s-per-profile wall-clock budget set in the Sprint 2 spec.
pub const BOOT_DEADLINE: Duration = Duration::from_secs(10);

/// The directory layout a gateway child expects — config + db rooted in a
/// tempdir that the test owns for the duration of the scenario.
///
/// Tests that only boot the gateway once can use [`InProcessGateway::start_local`],
/// which creates + owns an internal root.  Scenarios that need to restart
/// the gateway (to prove state persists across a crash) construct a
/// `GatewayRoot` first and then call [`InProcessGateway::start_with_root`]
/// twice against it — the tempdir lives on the test's stack and outlives
/// both gateway handles.
pub struct GatewayRoot {
    /// Held purely for Drop side-effect — dropping removes the tempdir.
    pub dir: tempfile::TempDir,
    /// Path to the `sera.yaml` the gateway should load.
    pub config_path: PathBuf,
    /// Expected path where SQLite state will be written (next to cwd).
    pub db_path: PathBuf,
}

impl GatewayRoot {
    /// Create a fresh tempdir with a minimal single-agent manifest.
    ///
    /// The generated `sera.yaml` points at `llm_base_url` for the provider's
    /// `base_url` and uses `model` for both `default_model` and the agent's
    /// `model` field.  Use [`resolve_model_env`] to pick `model` when the
    /// caller doesn't have a specific value in mind.
    pub fn new_local(llm_base_url: &str, model: &str) -> Result<Self> {
        Self::new_with_manifest(&minimal_sera_yaml(llm_base_url, model))
    }

    /// Create a fresh tempdir containing a caller-supplied `sera.yaml` body.
    ///
    /// Use this when a scenario needs a specific manifest shape — e.g. two
    /// agents for the multi-agent load test, or an agent with a `policyRef`
    /// for the capability-policy scenarios.  [`minimal_sera_yaml`] and
    /// [`multi_agent_sera_yaml`] are the convenience renderers.
    pub fn new_with_manifest(manifest: &str) -> Result<Self> {
        let tempdir = tempfile::Builder::new()
            .prefix("sera-e2e-")
            .tempdir()
            .context("creating tempdir for GatewayRoot")?;
        let config_path = tempdir.path().join("sera.yaml");
        std::fs::write(&config_path, manifest)
            .context("writing sera.yaml into tempdir")?;
        let db_path = tempdir.path().join("sera.db");
        Ok(Self { dir: tempdir, config_path, db_path })
    }
}

/// A spawned `sera-gateway` child, bound to a random loopback port, backed by
/// a throw-away SQLite database in a [`tempfile::TempDir`].
///
/// Dropping the handle without calling [`Self::shutdown`] will send `SIGKILL`
/// via tokio's `Child::kill` drop behaviour — deliberate: a hung test must
/// not leak a process into the developer's `ps` list.  Use `shutdown()` for
/// the well-behaved path, which sends SIGTERM and waits for graceful exit.
pub struct InProcessGateway {
    /// HTTP base URL — always `http://127.0.0.1:<port>` with no trailing slash.
    pub base_url: String,
    /// Absolute path to the SQLite file the gateway writes audit and
    /// transcript rows into.  Tests query this directly for assertions the
    /// HTTP API doesn't expose (the autonomous gateway has no
    /// `/api/audit` endpoint in this build).
    pub db_path: PathBuf,
    /// Absolute path to the `sera.yaml` the gateway loaded — occasionally
    /// useful for assertions over agent/connector manifests.
    pub config_path: PathBuf,
    /// Child process handle; owned here so shutdown can reap it.
    child: Child,
    /// When `Some`, this gateway owns the tempdir and will drop it after
    /// shutdown — that's the classic single-boot path.  When `None`, the
    /// caller owns the tempdir via a [`GatewayRoot`] and can start a new
    /// gateway against the same root for restart-semantics tests.
    _tempdir: Option<tempfile::TempDir>,
}

impl InProcessGateway {
    /// Boot a gateway in the "local" profile — SQLite only, no Postgres, no
    /// Centrifugo, no Discord connector, auth disabled (autonomous mode).
    ///
    /// Creates an internal tempdir with a fresh `sera.yaml`; the returned
    /// `InProcessGateway` owns the tempdir and removes it on Drop.  Use
    /// this for single-boot scenarios.  For multi-boot scenarios (restart
    /// tests) construct a [`GatewayRoot`] first and call
    /// [`Self::start_with_root`] instead.
    pub async fn start_local(
        gateway_bin: &Path,
        runtime_bin: &Path,
        llm_base_url: &str,
    ) -> Result<Self> {
        Self::start_local_with_env(gateway_bin, runtime_bin, llm_base_url, &[]).await
    }

    /// Boot a gateway with the same defaults as [`Self::start_local`] but
    /// with extra environment variables applied after the harness defaults.
    ///
    /// Use this for scenarios that need to override a default (e.g. set
    /// `SERA_ADMIN_SOCK` to a tempdir-scoped path for the kill-switch
    /// scenario, or unset `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE` by
    /// passing `("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE", "0")`).
    pub async fn start_local_with_env(
        gateway_bin: &Path,
        runtime_bin: &Path,
        llm_base_url: &str,
        extra_env: &[(&str, &str)],
    ) -> Result<Self> {
        let model = resolve_model_env();
        let root = GatewayRoot::new_local(llm_base_url, &model)?;
        let (child, base_url) = spawn_gateway(
            root.dir.path(),
            &root.config_path,
            gateway_bin,
            runtime_bin,
            llm_base_url,
            extra_env,
        )
        .await?;
        let gateway = Self {
            base_url,
            db_path: root.db_path.clone(),
            config_path: root.config_path.clone(),
            child,
            _tempdir: Some(root.dir),
        };
        gateway.wait_for_health().await?;
        Ok(gateway)
    }

    /// Boot a gateway against a caller-owned [`GatewayRoot`].
    ///
    /// The root's tempdir outlives this gateway (caller holds the `GatewayRoot`
    /// on the test stack), so two successive `start_with_root` calls against
    /// the same root share the same SQLite file — use this to assert
    /// persistence across restarts.
    pub async fn start_with_root(
        root: &GatewayRoot,
        gateway_bin: &Path,
        runtime_bin: &Path,
        llm_base_url: &str,
    ) -> Result<Self> {
        let (child, base_url) = spawn_gateway(
            root.dir.path(),
            &root.config_path,
            gateway_bin,
            runtime_bin,
            llm_base_url,
            &[],
        )
        .await?;
        let gateway = Self {
            base_url,
            db_path: root.db_path.clone(),
            config_path: root.config_path.clone(),
            child,
            _tempdir: None,
        };
        gateway.wait_for_health().await?;
        Ok(gateway)
    }

    /// Parse the address we bound to as a [`SocketAddr`] — handy when a test
    /// wants to exercise raw TCP or open a second connection of its own.
    pub fn socket_addr(&self) -> SocketAddr {
        // Invariant: base_url is always "http://127.0.0.1:<port>".
        let host_port = self.base_url.trim_start_matches("http://");
        host_port
            .parse()
            .unwrap_or_else(|e| panic!("base_url {} is not a SocketAddr: {e}", self.base_url))
    }

    async fn wait_for_health(&self) -> Result<()> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .context("building health-check client")?;
        let url = format!("{}/api/health", self.base_url);

        let deadline = Instant::now() + BOOT_DEADLINE;
        let mut last_err: Option<String> = None;
        while Instant::now() < deadline {
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                Ok(resp) => last_err = Some(format!("HTTP {}", resp.status())),
                Err(e) => last_err = Some(e.to_string()),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Err(anyhow!(
            "gateway /api/health did not return 200 within {:?}: {}",
            BOOT_DEADLINE,
            last_err.unwrap_or_else(|| "no successful response".into())
        ))
    }

    /// Send SIGTERM (via tokio's `Child::kill` fallback on non-Unix) and wait
    /// for the child to exit.  Always returns after at most a few seconds —
    /// the gateway's own drain deadline is 30s, but we cut our patience to
    /// 5s here so a stuck gateway in CI fails the test rather than hanging
    /// it.
    pub async fn shutdown(mut self) -> Result<()> {
        #[cfg(unix)]
        if let Some(pid) = self.child.id() {
            // SAFETY: libc::kill is declared with standard FFI contract and
            // accepts arbitrary pids; the worst case is ESRCH / EPERM which
            // we ignore.
            unsafe {
                let _ = libc_kill(pid as i32, 15);
            }
        }

        let wait = tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await;
        match wait {
            Ok(Ok(status)) => {
                tracing::info!(?status, "gateway exited cleanly");
                Ok(())
            }
            Ok(Err(e)) => Err(anyhow!("waiting for gateway child: {e}")),
            Err(_) => {
                let _ = self.child.start_kill();
                let _ = self.child.wait().await;
                Err(anyhow!("gateway did not exit within 5s, force-killed"))
            }
        }
    }
}

/// Spawn `sera-gateway start --config X --port P` rooted in `root_dir`, drain
/// stdio into tracing, and return the child handle + picked port's base URL.
/// Does not poll for health — caller is expected to call `wait_for_health`.
///
/// `extra_env` entries are applied after the harness defaults, so a scenario
/// can override any default (e.g. set `SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE`
/// to `"0"` for the constitutional-gate scenario, or set `SERA_ADMIN_SOCK` to
/// a tempdir path for the kill-switch scenario).
async fn spawn_gateway(
    root_dir: &Path,
    config_path: &Path,
    gateway_bin: &Path,
    runtime_bin: &Path,
    llm_base_url: &str,
    extra_env: &[(&str, &str)],
) -> Result<(Child, String)> {
    let port = pick_free_port()?;
    let addr = format!("127.0.0.1:{port}");
    let base_url = format!("http://{addr}");

    let mut cmd = Command::new(gateway_bin);
    cmd.arg("start")
        .arg("--config")
        .arg(config_path)
        .arg("--port")
        .arg(port.to_string())
        .current_dir(root_dir)
        .env("SERA_RUNTIME_BIN", runtime_bin)
        .env("LLM_BASE_URL", llm_base_url)
        // Pin RUST_LOG low: we want test output clean, not a firehose of
        // info-level gateway lifecycle lines.  Raise to `debug` by
        // setting `SERA_E2E_LOG=debug` if a scenario needs it.
        .env(
            "RUST_LOG",
            std::env::var("SERA_E2E_LOG").unwrap_or_else(|_| "warn".into()),
        )
        // Hard-disable every optional external backend so a bare dev box
        // can pass without Postgres, Redis, or Centrifugo running.
        .env_remove("DATABASE_URL")
        .env_remove("CENTRIFUGO_URL")
        .env_remove("SERA_API_KEY")
        // Mirror the permissive-dev flag that `sera start --local` sets:
        // without it the ConstitutionalGate hook intercepts every turn with
        // "[interrupted: no ConstitutionalGate policy installed]" because
        // the harness manifest does not declare a policy file.  Integration
        // tests explicitly opt into the permissive mode.
        .env("SERA_ALLOW_MISSING_CONSTITUTIONAL_GATE", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    let mut child = cmd
        .spawn()
        .with_context(|| format!("spawning gateway binary at {gateway_bin:?}"))?;

    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(pipe_to_tracing("gw-out", stdout));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(pipe_to_tracing("gw-err", stderr));
    }

    Ok((child, base_url))
}

/// Resolve the model identifier to use in the generated `sera.yaml`.
///
/// Reads `SERA_E2E_MODEL` from the environment.  If the variable is set and
/// non-empty, that value is used for both `default_model` and the agent's
/// `model` field.  If unset or empty, falls back to `"e2e-mock"`, which is
/// the model name accepted by the built-in wiremock fallback.
pub fn resolve_model_env() -> String {
    match std::env::var("SERA_E2E_MODEL") {
        Ok(v) if !v.is_empty() => v,
        _ => "e2e-mock".to_owned(),
    }
}

/// Minimal valid manifest set — one Instance, one Provider, one Agent.
///
/// The provider's `base_url` is templated to the caller-supplied LLM URL
/// because the autonomous gateway pulls this out of the manifest and
/// injects it into every spawned `sera-runtime` child's env as
/// `LLM_BASE_URL`.  The `model` parameter controls the `default_model` on
/// the provider and the agent's `model` field — use [`resolve_model_env`] to
/// obtain the right value for the current test environment.  We deliberately
/// keep the manifest otherwise boring: one agent, no connectors, no tools,
/// no persona beyond a short anchor.
pub fn minimal_sera_yaml(llm_base_url: &str, model: &str) -> String {
    format!(
        r#"apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: sera-e2e
spec: {{}}
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: mock-openai
spec:
  kind: openai-compatible
  base_url: "{llm_base_url}"
  default_model: {model}
---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: sera
spec:
  provider: mock-openai
  model: {model}
  persona:
    immutable_anchor: |
      You are a SERA e2e test persona. Reply briefly.
"#
    )
}

/// Multi-agent manifest — one Instance, one Provider, N Agents named by the
/// caller.  Each agent shares the same provider + model; persona anchors
/// differ so the manifest-loader's agent-ordering and list endpoint can be
/// asserted on distinct payloads.
///
/// Use this for S2.x manifest scenarios that need more than one agent.
pub fn multi_agent_sera_yaml(llm_base_url: &str, model: &str, agent_names: &[&str]) -> String {
    let mut out = format!(
        r#"apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: sera-e2e
spec: {{}}
---
apiVersion: sera.dev/v1
kind: Provider
metadata:
  name: mock-openai
spec:
  kind: openai-compatible
  base_url: "{llm_base_url}"
  default_model: {model}
"#
    );
    for name in agent_names {
        out.push_str(&format!(
            r#"---
apiVersion: sera.dev/v1
kind: Agent
metadata:
  name: {name}
spec:
  provider: mock-openai
  model: {model}
  persona:
    immutable_anchor: |
      You are {name}, a SERA e2e test persona. Reply briefly.
"#
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::resolve_model_env;

    // NOTE: This test mutates the process environment, which is not safe when
    // multiple threads read env vars concurrently.  Cargo runs unit tests in a
    // single binary with multiple threads by default.  Mark #[ignore] so the
    // test must be opted into explicitly with `cargo test -- --ignored`, which
    // the caller can run single-threaded via `cargo test -- --ignored
    // --test-threads=1`.
    #[test]
    #[ignore = "mutates process env; run with --test-threads=1 -- --ignored"]
    fn resolve_model_env_returns_env_var_when_set() {
        // SAFETY: test is #[ignore]-d; caller must use --test-threads=1.
        unsafe { std::env::set_var("SERA_E2E_MODEL", "my-real-model") };
        let result = resolve_model_env();
        unsafe { std::env::remove_var("SERA_E2E_MODEL") };
        assert_eq!(result, "my-real-model");
    }

    #[test]
    #[ignore = "mutates process env; run with --test-threads=1 -- --ignored"]
    fn resolve_model_env_falls_back_when_unset() {
        unsafe { std::env::remove_var("SERA_E2E_MODEL") };
        assert_eq!(resolve_model_env(), "e2e-mock");
    }

    #[test]
    #[ignore = "mutates process env; run with --test-threads=1 -- --ignored"]
    fn resolve_model_env_falls_back_when_empty() {
        unsafe { std::env::set_var("SERA_E2E_MODEL", "") };
        let result = resolve_model_env();
        unsafe { std::env::remove_var("SERA_E2E_MODEL") };
        assert_eq!(result, "e2e-mock");
    }
}

/// Bind `127.0.0.1:0` and read back the OS-assigned port, then release the
/// socket.  This has a small TOCTOU window vs. other processes on the box,
/// but the race is benign for a single-threaded test runner: the gateway
/// binds the same port moments later, and the OS does not recycle the port
/// number within that window.
pub fn pick_free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("binding 127.0.0.1:0 for port pick")?;
    let port = listener
        .local_addr()
        .context("reading local_addr of port-picker listener")?
        .port();
    drop(listener);
    Ok(port)
}

/// Relay every line from the child's stdio into `tracing` so a failing test
/// shows the gateway's log output without the operator having to re-run
/// manually.  Each line is emitted at `info` level prefixed by `tag`.
async fn pipe_to_tracing<R>(tag: &'static str, reader: R)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        tracing::info!(target: "sera_e2e_harness::gateway", "{tag}: {line}");
    }
}

// ── Minimal libc::kill shim ─────────────────────────────────────────────────
//
// We intentionally avoid pulling `libc` or `nix` into the dep graph for one
// syscall.  The FFI declaration below is the standard POSIX signature.
// Windows is stubbed to a no-op so the crate still compiles; on Windows the
// `tokio::process::Child::kill` (from `kill_on_drop`) does the equivalent
// work via TerminateProcess.

#[cfg(unix)]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
#[inline]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    unsafe { kill(pid, sig) }
}

#[cfg(not(unix))]
#[inline]
#[allow(dead_code)]
unsafe fn libc_kill(_pid: i32, _sig: i32) -> i32 {
    0
}

// ── Small assertion helpers ─────────────────────────────────────────────────
//
// These wrap rusqlite queries into pass/fail assertions tests can call from
// a single line.  They deliberately live here (rather than in-test) so
// future scenarios can reuse them without copy-paste.

/// Count rows in the audit_log whose event_type matches `event`.  Returns 0
/// if the database does not exist yet — a common state during the narrow
/// window between gateway boot and first turn.  Returns an error for any
/// other DB failure so callers see the real cause rather than a silent zero.
pub fn count_audit_rows(db_path: &Path, event: &str) -> Result<i64> {
    if !db_path.exists() {
        return Ok(0);
    }
    let conn =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening {db_path:?} for audit count"))?;
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE event_type = ?1",
            rusqlite::params![event],
            |row| row.get(0),
        )
        .map_err(|e| anyhow::anyhow!("querying audit_log in {db_path:?}: {e}"))?;
    Ok(count)
}

/// Count transcript rows for a given session id, filtered by role.  In the
/// autonomous gateway's SQLite schema, the assistant's reply to a user turn
/// is persisted here — it is the closest analogue to a "MemoryBlock segment
/// landed" assertion the Sprint 2 spec calls for.  Returns an error for any
/// DB failure so callers see the real cause rather than a silent zero.
pub fn count_transcript_rows(db_path: &Path, session_id: &str, role: &str) -> Result<i64> {
    if !db_path.exists() {
        return Ok(0);
    }
    let conn =
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("opening {db_path:?} for transcript count"))?;
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM transcript WHERE session_id = ?1 AND role = ?2",
            rusqlite::params![session_id, role],
            |row| row.get(0),
        )
        .map_err(|e| anyhow::anyhow!("querying transcript in {db_path:?}: {e}"))?;
    Ok(count)
}

// Re-export rusqlite so test authors don't need to add it as a direct dep
// just to build a raw query outside the count helpers.
pub use rusqlite;
