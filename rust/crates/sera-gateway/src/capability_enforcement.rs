//! Capability policy enforcement — dispatch-time policy checks for tool calls
//! (sera-ifjl).
//!
//! Loads `CapabilityPolicy` YAML files from a configured directory (default
//! `./capability-policies/`, override via `SERA_CAPABILITY_POLICIES_DIR`),
//! binds each loaded policy to the agents whose manifest declares a matching
//! `policyRef`, and exposes `CapabilityRegistry::check(agent_id, tool_name)`
//! for the tool-dispatch path.
//!
//! Semantics:
//! * Agents whose manifest has no `policy_ref` bypass the registry check
//!   (permissive by default — preserves existing MVS behaviour).
//! * Agents whose manifest declares a `policy_ref` but the policy is missing
//!   from the directory cause `CapabilityRegistry::load_and_bind` to return
//!   an error — startup **fails closed** rather than silently allowing every
//!   tool. This matches the P1 security posture in sera-ifjl.
//! * Once bound, every `check(agent_id, tool)` consults the policy's allowed
//!   tool list. A non-match returns `PolicyDenial` and the caller is
//!   responsible for emitting the audit entry and surfacing the denial.
//!
//! Policy YAML format (minimum viable subset):
//!
//! ```yaml
//! apiVersion: sera/v1
//! kind: CapabilityPolicy
//! metadata:
//!   name: tier-1
//! allowedTools:
//!   - memory_read
//!   - memory_write
//! ```
//!
//! Richer existing policies (see `capability-policies/read-only.yaml` etc.)
//! are also accepted — the loader is permissive on unknown fields and only
//! requires `metadata.name` and an `allowedTools` list. When an existing
//! policy has no `allowedTools` key we also accept a top-level `tools.allow`
//! shape (mirroring the broader CapabilityPolicy schema) so this module can
//! be pointed at the real policies directory without requiring a rewrite.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

/// Environment variable overriding the policies directory.
pub const POLICIES_DIR_ENV: &str = "SERA_CAPABILITY_POLICIES_DIR";

/// Default relative directory used when `SERA_CAPABILITY_POLICIES_DIR` is unset.
pub const DEFAULT_POLICIES_DIR: &str = "./capability-policies";

/// Errors produced when loading policies from disk or binding agents.
#[derive(Debug, Error)]
pub enum CapabilityRegistryError {
    #[error("capability-policies directory not found: {0}")]
    DirNotFound(PathBuf),
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse capability policy {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("duplicate capability policy name '{name}' (found in {first} and {second})")]
    DuplicateName {
        name: String,
        first: PathBuf,
        second: PathBuf,
    },
    #[error(
        "agent '{agent}' references capability policy '{policy_ref}' but no such policy was loaded from {dir}"
    )]
    MissingPolicyForAgent {
        agent: String,
        policy_ref: String,
        dir: PathBuf,
    },
}

/// Denial returned by `CapabilityRegistry::check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDenial {
    pub agent_id: String,
    pub tool_name: String,
    pub policy_name: String,
    pub reason: String,
}

/// Raw on-disk shape of a capability policy.
///
/// Only the fields used for tool-name enforcement are captured; everything
/// else (network, exec, filesystem scopes, …) is intentionally ignored for
/// the MVS surgical fix. `serde(default)` + `deny_unknown_fields = false`
/// (the default) means unrelated keys are tolerated.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPolicyFile {
    metadata: RawPolicyMetadata,
    /// Minimal / explicit allowed tool list. Spelled `allowedTools` in the
    /// YAML to match the existing capability-policy schema (see
    /// `capability-policies/README.md`).
    #[serde(default)]
    allowed_tools: Option<Vec<String>>,
    /// Compatibility with the richer `CapabilityPolicy` shape already
    /// present under `capability-policies/` — if `capabilities.tools.allow`
    /// is present it is folded into the allowed-tool set. The existing
    /// starter policies (`full-dev.yaml`, …) use this shape for their
    /// `exec` section rather than naming SERA tools directly; callers that
    /// want tool-name enforcement must opt in via `allowedTools`.
    #[serde(default)]
    capabilities: Option<RawCapabilities>,
}

#[derive(Debug, Deserialize)]
struct RawPolicyMetadata {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawCapabilities {
    #[serde(default)]
    tools: Option<RawTools>,
}

#[derive(Debug, Deserialize)]
struct RawTools {
    #[serde(default)]
    allow: Vec<String>,
}

/// A loaded capability policy, reduced to its dispatch-relevant fields.
#[derive(Debug, Clone)]
pub struct CapabilityPolicy {
    pub name: String,
    pub allowed_tools: HashSet<String>,
}

/// Registry of loaded capability policies, with agent→policy bindings.
#[derive(Debug, Default, Clone)]
pub struct CapabilityRegistry {
    /// Loaded policies, keyed by policy name (matches `metadata.name` and
    /// the `policyRef` value on agent manifests).
    policies: HashMap<String, CapabilityPolicy>,
    /// Bindings: agent id → policy name.
    ///
    /// Missing entries signal "no policy configured for this agent" — the
    /// registry is permissive in that case, preserving existing behaviour.
    bindings: HashMap<String, String>,
}

impl CapabilityRegistry {
    /// Create an empty registry. Useful for tests and for the no-policy
    /// default when the policies directory is missing or empty.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Number of loaded policies.
    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }

    /// Look up a loaded policy by name.
    pub fn policy(&self, name: &str) -> Option<&CapabilityPolicy> {
        self.policies.get(name)
    }

    /// Resolve the effective policy name for an agent, if any binding exists.
    pub fn policy_for_agent(&self, agent_id: &str) -> Option<&str> {
        self.bindings.get(agent_id).map(String::as_str)
    }

    /// Check whether `agent_id` is allowed to invoke `tool_name`.
    ///
    /// * Returns `Ok(())` if the agent has no policy binding — permissive
    ///   default, matching pre-sera-ifjl behaviour.
    /// * Returns `Ok(())` if the bound policy's allowed-tool set contains
    ///   `tool_name`.
    /// * Otherwise returns `Err(PolicyDenial)`.
    pub fn check(&self, agent_id: &str, tool_name: &str) -> Result<(), PolicyDenial> {
        let Some(policy_name) = self.bindings.get(agent_id) else {
            return Ok(());
        };
        // If the binding points at an unknown policy we treat that as a
        // denial — fail-closed. This path should not be reachable in
        // practice because `load_and_bind` refuses to bind missing policies,
        // but we defend against accidental direct construction.
        let Some(policy) = self.policies.get(policy_name) else {
            return Err(PolicyDenial {
                agent_id: agent_id.to_string(),
                tool_name: tool_name.to_string(),
                policy_name: policy_name.clone(),
                reason: format!(
                    "agent bound to policy '{policy_name}' but policy not loaded"
                ),
            });
        };
        if policy.allowed_tools.contains(tool_name) {
            Ok(())
        } else {
            Err(PolicyDenial {
                agent_id: agent_id.to_string(),
                tool_name: tool_name.to_string(),
                policy_name: policy_name.clone(),
                reason: format!(
                    "tool '{tool_name}' not in allowedTools for policy '{policy_name}'"
                ),
            })
        }
    }

    /// Resolve the policies directory from the environment, falling back to
    /// `DEFAULT_POLICIES_DIR`.
    pub fn resolve_policies_dir() -> PathBuf {
        std::env::var_os(POLICIES_DIR_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_POLICIES_DIR))
    }

    /// Load all policy YAMLs under `dir` and bind the supplied agent
    /// → `policy_ref` map. Agents with `policy_ref = None` produce no
    /// binding (permissive).
    ///
    /// Fails closed when any agent references a policy name that does not
    /// exist on disk.
    ///
    /// When `dir` is missing and there are no agents with a `policy_ref`
    /// binding, this returns an empty registry. When `dir` is missing and
    /// an agent has a `policy_ref`, this returns `MissingPolicyForAgent`.
    pub fn load_and_bind<I, A, P>(
        dir: &Path,
        agents: I,
    ) -> Result<Self, CapabilityRegistryError>
    where
        I: IntoIterator<Item = (A, Option<P>)>,
        A: Into<String>,
        P: Into<String>,
    {
        let policies = if dir.exists() {
            load_policies(dir)?
        } else {
            HashMap::new()
        };

        let mut bindings: HashMap<String, String> = HashMap::new();
        for (agent, policy_ref) in agents {
            let agent = agent.into();
            let Some(policy_ref) = policy_ref else {
                continue;
            };
            let policy_ref = policy_ref.into();
            if !policies.contains_key(&policy_ref) {
                return Err(CapabilityRegistryError::MissingPolicyForAgent {
                    agent,
                    policy_ref,
                    dir: dir.to_path_buf(),
                });
            }
            bindings.insert(agent, policy_ref);
        }

        Ok(Self {
            policies,
            bindings,
        })
    }

    /// Test / doctor helper: construct a registry directly from in-memory
    /// policies and bindings, skipping disk IO.
    pub fn from_parts(
        policies: Vec<CapabilityPolicy>,
        bindings: HashMap<String, String>,
    ) -> Self {
        let policies = policies
            .into_iter()
            .map(|p| (p.name.clone(), p))
            .collect();
        Self {
            policies,
            bindings,
        }
    }
}

/// Walk `dir` and load every `.yaml` / `.yml` file as a `CapabilityPolicy`.
fn load_policies(
    dir: &Path,
) -> Result<HashMap<String, CapabilityPolicy>, CapabilityRegistryError> {
    let entries =
        std::fs::read_dir(dir).map_err(|source| CapabilityRegistryError::Io {
            path: dir.to_path_buf(),
            source,
        })?;

    let mut loaded: HashMap<String, CapabilityPolicy> = HashMap::new();
    let mut origins: HashMap<String, PathBuf> = HashMap::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !is_yaml(&path) {
            continue;
        }
        // Skip files that aren't CapabilityPolicy documents (e.g. rbac.conf
        // is in the same dir in the example layout). We do a cheap prefix
        // sniff rather than parsing every file twice.
        let content = std::fs::read_to_string(&path).map_err(|source| {
            CapabilityRegistryError::Io {
                path: path.clone(),
                source,
            }
        })?;
        if !looks_like_capability_policy(&content) {
            continue;
        }

        let raw: RawPolicyFile = serde_yaml::from_str(&content).map_err(|source| {
            CapabilityRegistryError::Parse {
                path: path.clone(),
                source,
            }
        })?;

        let name = raw.metadata.name;
        let mut allowed: HashSet<String> = HashSet::new();
        if let Some(list) = raw.allowed_tools {
            allowed.extend(list);
        }
        if let Some(caps) = raw.capabilities
            && let Some(tools) = caps.tools
        {
            allowed.extend(tools.allow);
        }

        let policy = CapabilityPolicy {
            name: name.clone(),
            allowed_tools: allowed,
        };

        if let Some(existing) = origins.get(&name) {
            return Err(CapabilityRegistryError::DuplicateName {
                name,
                first: existing.clone(),
                second: path.clone(),
            });
        }
        origins.insert(name.clone(), path);
        loaded.insert(name, policy);
    }

    Ok(loaded)
}

fn is_yaml(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yaml") | Some("yml")
    )
}

/// Cheap prefix sniff: a CapabilityPolicy document mentions
/// `kind: CapabilityPolicy` near the top. This keeps the loader from
/// choking on unrelated YAML files that may live in the same directory
/// (e.g. `rbac.csv` adjacents, alternate schemas). Any file that passes
/// the sniff is parsed strictly — parse failures still surface.
fn looks_like_capability_policy(content: &str) -> bool {
    content
        .lines()
        .take(20)
        .any(|l| l.trim_start().starts_with("kind:") && l.contains("CapabilityPolicy"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    const TIER1_YAML: &str = r#"
apiVersion: sera/v1
kind: CapabilityPolicy
metadata:
  name: tier-1
allowedTools:
  - memory_read
"#;

    const TIER2_YAML: &str = r#"
apiVersion: sera/v1
kind: CapabilityPolicy
metadata:
  name: tier-2
allowedTools:
  - memory_read
  - memory_write
  - shell
"#;

    #[test]
    fn empty_registry_permits_everything() {
        let reg = CapabilityRegistry::empty();
        assert!(reg.check("any-agent", "any-tool").is_ok());
    }

    #[test]
    fn unbound_agent_is_permissive() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "tier-1.yaml", TIER1_YAML);
        let reg = CapabilityRegistry::load_and_bind::<_, String, String>(
            dir.path(),
            Vec::<(String, Option<String>)>::new(),
        )
        .unwrap();
        assert!(reg.check("unbound", "shell").is_ok());
    }

    #[test]
    fn bound_agent_allows_listed_tool() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "tier-2.yaml", TIER2_YAML);
        let reg = CapabilityRegistry::load_and_bind(
            dir.path(),
            vec![(String::from("alice"), Some(String::from("tier-2")))],
        )
        .unwrap();
        assert!(reg.check("alice", "shell").is_ok());
    }

    #[test]
    fn bound_agent_denies_missing_tool() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "tier-1.yaml", TIER1_YAML);
        let reg = CapabilityRegistry::load_and_bind(
            dir.path(),
            vec![(String::from("bob"), Some(String::from("tier-1")))],
        )
        .unwrap();
        let err = reg.check("bob", "shell").unwrap_err();
        assert_eq!(err.agent_id, "bob");
        assert_eq!(err.tool_name, "shell");
        assert_eq!(err.policy_name, "tier-1");
    }

    #[test]
    fn missing_policy_fails_closed_at_load() {
        let dir = TempDir::new().unwrap();
        // Only tier-1 exists; agent references tier-99.
        write(dir.path(), "tier-1.yaml", TIER1_YAML);
        let err = CapabilityRegistry::load_and_bind(
            dir.path(),
            vec![(String::from("carol"), Some(String::from("tier-99")))],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            CapabilityRegistryError::MissingPolicyForAgent { .. }
        ));
    }

    #[test]
    fn duplicate_policy_names_rejected() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "a.yaml", TIER1_YAML);
        write(dir.path(), "b.yaml", TIER1_YAML);
        let err =
            load_policies(dir.path()).expect_err("duplicate names must error");
        assert!(matches!(err, CapabilityRegistryError::DuplicateName { .. }));
    }

    #[test]
    fn non_capability_policy_yaml_is_ignored() {
        let dir = TempDir::new().unwrap();
        write(
            dir.path(),
            "unrelated.yaml",
            "kind: Something\nmetadata:\n  name: nope\n",
        );
        write(dir.path(), "tier-1.yaml", TIER1_YAML);
        let reg = CapabilityRegistry::load_and_bind::<_, String, String>(
            dir.path(),
            Vec::<(String, Option<String>)>::new(),
        )
        .unwrap();
        assert_eq!(reg.policy_count(), 1);
        assert!(reg.policy("tier-1").is_some());
    }

    #[test]
    fn resolve_policies_dir_uses_env_var() {
        // Isolate: set + read + unset. Tests share process so be conservative.
        let prev = std::env::var(POLICIES_DIR_ENV).ok();
        // SAFETY: test process, single-threaded within this test.
        unsafe {
            std::env::set_var(POLICIES_DIR_ENV, "/tmp/custom-policies");
        }
        assert_eq!(
            CapabilityRegistry::resolve_policies_dir(),
            PathBuf::from("/tmp/custom-policies")
        );
        unsafe {
            match prev {
                Some(v) => std::env::set_var(POLICIES_DIR_ENV, v),
                None => std::env::remove_var(POLICIES_DIR_ENV),
            }
        }
    }
}
