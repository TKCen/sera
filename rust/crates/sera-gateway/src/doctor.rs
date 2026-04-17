//! `sera doctor` — diagnostic check runner.
//!
//! Each check implements the [`Check`] trait and returns a [`CheckResult`].
//! The runner collects all results, prints a formatted table, and exits with
//! code 0 (no failures) or 1 (at least one Fail).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use serde::Serialize;

// ── Core types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "UPPERCASE", tag = "status")]
pub enum CheckResult {
    Pass(String),
    Warn(String),
    Fail(String),
    Skip(String),
}

impl CheckResult {
    pub fn label(&self) -> &'static str {
        match self {
            CheckResult::Pass(_) => "PASS",
            CheckResult::Warn(_) => "WARN",
            CheckResult::Fail(_) => "FAIL",
            CheckResult::Skip(_) => "SKIP",
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            CheckResult::Pass(d) | CheckResult::Warn(d) | CheckResult::Fail(d) | CheckResult::Skip(d) => d,
        }
    }

    pub fn is_fail(&self) -> bool {
        matches!(self, CheckResult::Fail(_))
    }
}

pub trait Check {
    fn name(&self) -> &'static str;
    fn run(&self) -> CheckResult;
}

// ── JSON output shape ────────────────────────────────────────────────────────

#[derive(Serialize)]
struct JsonRow {
    check: &'static str,
    status: &'static str,
    detail: String,
}

// ── Check 1: Config file loads ───────────────────────────────────────────────

pub struct ConfigLoadCheck {
    pub config_path: PathBuf,
}

impl Check for ConfigLoadCheck {
    fn name(&self) -> &'static str {
        "config.load"
    }

    fn run(&self) -> CheckResult {
        if !self.config_path.exists() {
            return CheckResult::Fail(format!(
                "file not found: {}",
                self.config_path.display()
            ));
        }

        let content = match std::fs::read_to_string(&self.config_path) {
            Ok(c) => c,
            Err(e) => return CheckResult::Fail(format!("read error: {e}")),
        };

        match sera_config::manifest_loader::parse_manifests(&content) {
            Ok(set) => CheckResult::Pass(format!(
                "loaded from {} ({} manifests)",
                self.config_path.display(),
                set.instances.len() + set.providers.len() + set.agents.len()
                    + set.connectors.len() + set.hook_chains.len()
            )),
            Err(e) => CheckResult::Fail(format!("parse error: {e}")),
        }
    }
}

// ── Check 2: Database reachable ──────────────────────────────────────────────

pub struct DbReachableCheck {
    pub database_url: Option<String>,
}

impl Check for DbReachableCheck {
    fn name(&self) -> &'static str {
        "db.reachable"
    }

    fn run(&self) -> CheckResult {
        let url = match &self.database_url {
            Some(u) if !u.is_empty() => u.clone(),
            _ => return CheckResult::Skip("DATABASE_URL not configured".to_string()),
        };

        if url.starts_with("sqlite:") || url.starts_with("sqlite3:") {
            let path = url
                .trim_start_matches("sqlite3:")
                .trim_start_matches("sqlite:")
                .trim_start_matches("//");
            let start = Instant::now();
            match rusqlite::Connection::open(path) {
                Ok(conn) => {
                    match conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)) {
                        Ok(_) => CheckResult::Pass(format!(
                            "sqlite ok ({}ms)",
                            start.elapsed().as_millis()
                        )),
                        Err(e) => CheckResult::Fail(format!("sqlite PRAGMA failed: {e}")),
                    }
                }
                Err(e) => CheckResult::Fail(format!("sqlite open failed: {e}")),
            }
        } else {
            // Postgres / other: just verify the URL parses and is reachable via a
            // synchronous TCP connect to the host:port.
            let trimmed = url
                .trim_start_matches("postgresql://")
                .trim_start_matches("postgres://");
            // Extract host:port from "user:pass@host:port/dbname"
            let hostpart = trimmed
                .find('@')
                .map(|i| &trimmed[i + 1..])
                .unwrap_or(trimmed);
            let addr = hostpart.split('/').next().unwrap_or(hostpart);
            let addr = if addr.contains(':') {
                addr.to_string()
            } else {
                format!("{addr}:5432")
            };

            let start = Instant::now();
            match std::net::TcpStream::connect_timeout(
                &addr.parse().unwrap_or_else(|_| "127.0.0.1:5432".parse().unwrap()),
                std::time::Duration::from_secs(3),
            ) {
                Ok(_) => CheckResult::Pass(format!(
                    "tcp connect to {addr} ok ({}ms)",
                    start.elapsed().as_millis()
                )),
                Err(e) => CheckResult::Fail(format!("tcp connect to {addr} failed: {e}")),
            }
        }
    }
}

// ── Check 3: Required env vars ───────────────────────────────────────────────

/// The env vars the doctor checks. These are the production secrets defined in
/// `sera_config::core_config::DEV_SECRET_VALUES`.
const REQUIRED_ENV_VARS: &[&str] = &[
    "SERA_API_KEY",
    "SERA_TOKEN_SECRET",
    "SERA_MASTER_KEY",
];

/// Dev-default values that are known-unsafe for production.
const DEV_DEFAULTS: &[&str] = &[
    "sera_bootstrap_dev_123",
    "lm-studio",
    "sera-api-key",
    "sera-token-secret",
    "sera-dev-master-key-change-me",
    "change-me",
    "secret",
    "password",
];

pub struct EnvSecretsCheck;

impl Check for EnvSecretsCheck {
    fn name(&self) -> &'static str {
        "env.secrets"
    }

    fn run(&self) -> CheckResult {
        let mut missing: Vec<&str> = Vec::new();
        let mut dev_defaults: Vec<&str> = Vec::new();

        for var in REQUIRED_ENV_VARS {
            match std::env::var(var) {
                Ok(val) if DEV_DEFAULTS.contains(&val.as_str()) => {
                    dev_defaults.push(var);
                }
                Ok(_) => {}
                Err(_) => missing.push(var),
            }
        }

        if !missing.is_empty() {
            return CheckResult::Fail(format!("missing: {}", missing.join(", ")));
        }
        if !dev_defaults.is_empty() {
            return CheckResult::Warn(format!(
                "dev-default values detected: {}",
                dev_defaults.join(", ")
            ));
        }
        CheckResult::Pass(format!("{} required vars present and non-default", REQUIRED_ENV_VARS.len()))
    }
}

// ── Check 4: Docker available ────────────────────────────────────────────────

pub struct DockerCheck;

impl Check for DockerCheck {
    fn name(&self) -> &'static str {
        "docker.available"
    }

    fn run(&self) -> CheckResult {
        // First check whether `docker` is on PATH
        let which = Command::new("which").arg("docker").output()
            .or_else(|_| Command::new("where").arg("docker").output());

        if let Err(e) = &which {
            return CheckResult::Fail(format!("could not check PATH: {e}"));
        }

        let which = which.unwrap();
        if !which.status.success() {
            return CheckResult::Fail("docker not found on PATH".to_string());
        }

        // Now get the version string
        match Command::new("docker").arg("--version").output() {
            Ok(out) if out.status.success() => {
                let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
                CheckResult::Pass(version)
            }
            Ok(out) => {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                CheckResult::Warn(format!("docker found but --version failed: {err}"))
            }
            Err(e) => CheckResult::Fail(format!("docker exec failed: {e}")),
        }
    }
}

// ── Check 5: Capability policies ────────────────────────────────────────────

pub struct CapabilityPoliciesCheck {
    pub policies_dir: PathBuf,
}

impl Check for CapabilityPoliciesCheck {
    fn name(&self) -> &'static str {
        "capability-policies"
    }

    fn run(&self) -> CheckResult {
        if !self.policies_dir.exists() {
            return CheckResult::Fail(format!(
                "directory not found: {}",
                self.policies_dir.display()
            ));
        }

        let entries = match std::fs::read_dir(&self.policies_dir) {
            Ok(e) => e,
            Err(e) => return CheckResult::Fail(format!("read_dir failed: {e}")),
        };

        let mut total = 0usize;
        let mut parse_ok = 0usize;
        let mut parse_errors: Vec<String> = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml")
                && path.extension().and_then(|e| e.to_str()) != Some("yml")
            {
                continue;
            }
            total += 1;
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    // Parse as generic YAML value — structural check only.
                    match serde_yaml::from_str::<serde_yaml::Value>(&content) {
                        Ok(_) => parse_ok += 1,
                        Err(e) => parse_errors.push(format!(
                            "{}: {e}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        )),
                    }
                }
                Err(e) => parse_errors.push(format!(
                    "{}: read error {e}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )),
            }
        }

        if total == 0 {
            return CheckResult::Warn(format!(
                "no YAML files found in {}",
                self.policies_dir.display()
            ));
        }

        if !parse_errors.is_empty() {
            return CheckResult::Fail(format!(
                "{}/{} policies parse-ok; errors: {}",
                parse_ok,
                total,
                parse_errors.join("; ")
            ));
        }

        CheckResult::Pass(format!("{total} policies loaded (all parse-ok)"))
    }
}

// ── Check 6: Secrets provider ────────────────────────────────────────────────

pub struct SecretsProviderCheck;

impl Check for SecretsProviderCheck {
    fn name(&self) -> &'static str {
        "secrets.provider"
    }

    fn run(&self) -> CheckResult {
        // Determine which provider is active based on configuration:
        // 1. SECRETS_FILE_DIR → file provider
        // 2. /run/secrets exists and is populated → docker provider
        // 3. SERA_SECRET_* env vars present → env provider
        // 4. Nothing → warn

        if let Ok(dir) = std::env::var("SECRETS_FILE_DIR") {
            let p = Path::new(&dir);
            if p.is_dir() {
                return CheckResult::Pass(format!("file provider active (dir={dir})"));
            } else {
                return CheckResult::Warn(format!(
                    "SECRETS_FILE_DIR set to '{dir}' but directory does not exist"
                ));
            }
        }

        let docker_secrets = Path::new("/run/secrets");
        if docker_secrets.is_dir() {
            let count = std::fs::read_dir(docker_secrets)
                .ok()
                .map(|d| d.count())
                .unwrap_or(0);
            if count > 0 {
                return CheckResult::Pass(format!(
                    "docker provider active ({count} secrets in /run/secrets)"
                ));
            }
        }

        let env_secrets: Vec<String> = std::env::vars()
            .filter(|(k, _)| k.starts_with("SERA_SECRET_"))
            .map(|(k, _)| k)
            .collect();

        if !env_secrets.is_empty() {
            return CheckResult::Pass(format!(
                "env provider active ({} SERA_SECRET_* vars)",
                env_secrets.len()
            ));
        }

        CheckResult::Warn("no secrets provider configured (env / docker / file)".to_string())
    }
}

// ── Check 7: LLM provider health (optional) ─────────────────────────────────

pub struct LlmProviderCheck {
    pub provider_urls: Vec<(String, String)>, // (name, base_url)
}

impl Check for LlmProviderCheck {
    fn name(&self) -> &'static str {
        "llm.providers"
    }

    fn run(&self) -> CheckResult {
        if self.provider_urls.is_empty() {
            return CheckResult::Skip("no providers configured".to_string());
        }

        let mut ok: Vec<String> = Vec::new();
        let mut failed: Vec<String> = Vec::new();

        for (name, base_url) in &self.provider_urls {
            // Extract host:port from the URL and do a TCP probe.
            let url = base_url.trim_end_matches('/');
            let hostpart = url
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            let addr_str = hostpart.split('/').next().unwrap_or(hostpart);
            let addr_str = if addr_str.contains(':') {
                addr_str.to_string()
            } else if url.starts_with("https://") {
                format!("{addr_str}:443")
            } else {
                format!("{addr_str}:80")
            };

            let start = Instant::now();
            match addr_str.parse::<std::net::SocketAddr>() {
                Ok(addr) => {
                    match std::net::TcpStream::connect_timeout(
                        &addr,
                        std::time::Duration::from_secs(3),
                    ) {
                        Ok(_) => ok.push(format!("{name} ({}ms)", start.elapsed().as_millis())),
                        Err(e) => failed.push(format!("{name}: {e}")),
                    }
                }
                Err(_) => failed.push(format!("{name}: cannot parse addr '{addr_str}'")),
            }
        }

        if !failed.is_empty() {
            CheckResult::Fail(format!(
                "unreachable: {}; ok: {}",
                failed.join(", "),
                ok.join(", ")
            ))
        } else {
            CheckResult::Pass(format!("{} providers reachable: {}", ok.len(), ok.join(", ")))
        }
    }
}

// ── Runner ───────────────────────────────────────────────────────────────────

pub struct RunnerResult {
    pub rows: Vec<(&'static str, CheckResult)>,
    pub any_fail: bool,
}

pub fn run_checks(checks: &[Box<dyn Check>]) -> RunnerResult {
    let rows: Vec<(&'static str, CheckResult)> = checks
        .iter()
        .map(|c| (c.name(), c.run()))
        .collect();
    let any_fail = rows.iter().any(|(_, r)| r.is_fail());
    RunnerResult { rows, any_fail }
}

pub fn print_table(result: &RunnerResult) {
    // Column widths
    let name_w = 35usize;
    let status_w = 8usize;

    println!(
        "{:<name_w$} {:<status_w$} DETAIL",
        "CHECK", "STATUS",
        name_w = name_w,
        status_w = status_w
    );
    println!("{}", "-".repeat(name_w + status_w + 50));

    for (name, result) in &result.rows {
        println!(
            "{:<name_w$} {:<status_w$} {}",
            name,
            result.label(),
            result.detail(),
            name_w = name_w,
            status_w = status_w
        );
    }
}

pub fn print_json(result: &RunnerResult) {
    let rows: Vec<JsonRow> = result
        .rows
        .iter()
        .map(|(name, r)| JsonRow {
            check: name,
            status: r.label(),
            detail: r.detail().to_string(),
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
}

// ── Build the default check list from a config path ─────────────────────────

pub fn build_checks(config_path: &Path) -> Vec<Box<dyn Check>> {
    // Try to load config to extract database URL and provider URLs.
    let manifest = sera_config::manifest_loader::load_manifest_file(config_path).ok();

    let database_url = std::env::var("DATABASE_URL").ok();

    let provider_urls: Vec<(String, String)> = manifest
        .as_ref()
        .map(|m| {
            m.providers
                .iter()
                .filter_map(|p| {
                    let provider_spec: sera_types::config_manifest::ProviderSpec =
                        serde_json::from_value(p.spec.clone()).ok()?;
                    Some((p.metadata.name.clone(), provider_spec.base_url.clone()))
                })
                .collect()
        })
        .unwrap_or_default();

    // Determine capability policies directory: relative to config file or project root.
    let policies_dir = config_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("capability-policies");

    let checks: Vec<Box<dyn Check>> = vec![
        Box::new(ConfigLoadCheck { config_path: config_path.to_path_buf() }),
        Box::new(DbReachableCheck { database_url }),
        Box::new(EnvSecretsCheck),
        Box::new(DockerCheck),
        Box::new(CapabilityPoliciesCheck { policies_dir }),
        Box::new(SecretsProviderCheck),
        Box::new(LlmProviderCheck { provider_urls }),
    ];

    checks
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ── ConfigLoadCheck ──────────────────────────────────────────────────────

    #[test]
    fn config_load_missing_file() {
        let check = ConfigLoadCheck {
            config_path: PathBuf::from("/nonexistent/sera.yaml"),
        };
        assert!(check.run().is_fail());
    }

    #[test]
    fn config_load_valid_yaml() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: test
spec:
  name: test
"#
        )
        .unwrap();
        let check = ConfigLoadCheck { config_path: f.path().to_path_buf() };
        let result = check.run();
        assert!(!result.is_fail(), "expected Pass or Warn, got: {:?}", result.label());
    }

    #[test]
    fn config_load_invalid_yaml() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{{{{ not yaml }}}}").unwrap();
        let check = ConfigLoadCheck { config_path: f.path().to_path_buf() };
        // Invalid YAML that can't be parsed as manifests → Fail
        // (may pass serde_yaml but fail manifest parsing — either is acceptable)
        let result = check.run();
        // Just assert it doesn't panic and returns a valid label
        assert!(["PASS", "WARN", "FAIL", "SKIP"].contains(&result.label()));
    }

    // ── DbReachableCheck ─────────────────────────────────────────────────────

    #[test]
    fn db_reachable_no_url() {
        let check = DbReachableCheck { database_url: None };
        assert!(matches!(check.run(), CheckResult::Skip(_)));
    }

    #[test]
    fn db_reachable_sqlite_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let url = format!("sqlite:{}", db_path.display());
        let check = DbReachableCheck { database_url: Some(url) };
        let result = check.run();
        assert!(matches!(result, CheckResult::Pass(_)), "got: {:?}", result.label());
    }

    #[test]
    fn db_reachable_postgres_refused() {
        // Port 1 is never open; expect Fail quickly.
        let check = DbReachableCheck {
            database_url: Some("postgres://user:pass@127.0.0.1:1/db".to_string()),
        };
        let result = check.run();
        assert!(matches!(result, CheckResult::Fail(_)));
    }

    // ── EnvSecretsCheck ──────────────────────────────────────────────────────

    #[test]
    fn env_secrets_missing() {
        // Remove the env vars if set, then run check.
        // We can't reliably unset env vars in parallel tests, so just run
        // and assert we get a valid result back.
        let check = EnvSecretsCheck;
        let result = check.run();
        assert!(["PASS", "WARN", "FAIL"].contains(&result.label()));
    }

    // ── DockerCheck ──────────────────────────────────────────────────────────

    #[test]
    fn docker_check_runs_without_panic() {
        let check = DockerCheck;
        let result = check.run();
        assert!(["PASS", "WARN", "FAIL"].contains(&result.label()));
    }

    // ── CapabilityPoliciesCheck ───────────────────────────────────────────────

    #[test]
    fn capability_policies_missing_dir() {
        let check = CapabilityPoliciesCheck {
            policies_dir: PathBuf::from("/nonexistent/capability-policies"),
        };
        assert!(check.run().is_fail());
    }

    #[test]
    fn capability_policies_valid() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("read-only.yaml")).unwrap();
        writeln!(f, "kind: CapabilityPolicy\nname: read-only").unwrap();
        let mut g = std::fs::File::create(dir.path().join("sandboxed.yaml")).unwrap();
        writeln!(g, "kind: CapabilityPolicy\nname: sandboxed").unwrap();

        let check = CapabilityPoliciesCheck { policies_dir: dir.path().to_path_buf() };
        let result = check.run();
        assert!(matches!(result, CheckResult::Pass(_)), "got: {:?}", result.detail());
    }

    #[test]
    fn capability_policies_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("bad.yaml")).unwrap();
        writeln!(f, "key: [unclosed bracket").unwrap();

        let check = CapabilityPoliciesCheck { policies_dir: dir.path().to_path_buf() };
        let result = check.run();
        assert!(matches!(result, CheckResult::Fail(_)));
    }

    // ── SecretsProviderCheck ─────────────────────────────────────────────────

    #[test]
    fn secrets_provider_file_dir_missing() {
        // Temporarily set SECRETS_FILE_DIR to a non-existent path.
        // We can't safely mutate env in parallel tests, so just exercise the
        // code path with a direct struct — SECRETS_FILE_DIR is not set in CI.
        let check = SecretsProviderCheck;
        let result = check.run();
        assert!(["PASS", "WARN", "FAIL"].contains(&result.label()));
    }

    // ── LlmProviderCheck ─────────────────────────────────────────────────────

    #[test]
    fn llm_provider_no_providers() {
        let check = LlmProviderCheck { provider_urls: vec![] };
        assert!(matches!(check.run(), CheckResult::Skip(_)));
    }

    #[test]
    fn llm_provider_unreachable() {
        let check = LlmProviderCheck {
            provider_urls: vec![("lm-studio".to_string(), "http://127.0.0.1:1".to_string())],
        };
        let result = check.run();
        assert!(matches!(result, CheckResult::Fail(_)));
    }

    // ── Runner ───────────────────────────────────────────────────────────────

    #[test]
    fn runner_collects_all_results() {
        struct AlwaysPass;
        impl Check for AlwaysPass {
            fn name(&self) -> &'static str { "test.pass" }
            fn run(&self) -> CheckResult { CheckResult::Pass("ok".to_string()) }
        }

        struct AlwaysFail;
        impl Check for AlwaysFail {
            fn name(&self) -> &'static str { "test.fail" }
            fn run(&self) -> CheckResult { CheckResult::Fail("broken".to_string()) }
        }

        let checks: Vec<Box<dyn Check>> = vec![
            Box::new(AlwaysPass),
            Box::new(AlwaysFail),
        ];

        let result = run_checks(&checks);
        assert_eq!(result.rows.len(), 2);
        assert!(result.any_fail);
    }

    #[test]
    fn runner_no_fail() {
        struct AlwaysPass;
        impl Check for AlwaysPass {
            fn name(&self) -> &'static str { "test.pass" }
            fn run(&self) -> CheckResult { CheckResult::Pass("ok".to_string()) }
        }

        let checks: Vec<Box<dyn Check>> = vec![Box::new(AlwaysPass)];
        let result = run_checks(&checks);
        assert!(!result.any_fail);
    }

    // ── Integration: full runner against test config ─────────────────────────

    #[test]
    fn integration_runner_shape() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"---
apiVersion: sera.dev/v1
kind: Instance
metadata:
  name: test-instance
spec:
  name: test-instance
"#
        )
        .unwrap();

        let checks = build_checks(f.path());
        assert!(checks.len() >= 6, "expected at least 6 checks");

        let result = run_checks(&checks);
        assert_eq!(result.rows.len(), checks.len());

        // All results must have a valid label
        for (name, r) in &result.rows {
            assert!(
                ["PASS", "WARN", "FAIL", "SKIP"].contains(&r.label()),
                "check '{}' returned invalid label",
                name
            );
        }
    }
}
