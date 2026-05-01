//! BYOH harness tool-catalog reconciliation (bead sera-w2dh).
//!
//! Pattern A only — Pattern B sessions do not share a tool catalog with the
//! gateway. SPEC-tools §13a / §717 says when a BYOH harness (Claude Code,
//! Codex, Hermes, …) registers, the gateway augments the harness's tool
//! schema with gateway-managed tools. The spec leaves the collision
//! algorithm undefined; without one a harness with its own `Read` tool and a
//! gateway tool of the same purpose (e.g. `read_file`) can produce silent
//! dual-dispatch, capability laundering (the harness uses its internal
//! implementation, bypassing gateway AuthZ + audit), or argument-shape
//! mismatch on alias.
//!
//! This module is the deterministic reconciliation policy. It is a pure
//! function on catalog types; the registration endpoint that drives it lands
//! in a follow-up bead — see
//! `artifacts/reports/claude-burn/w2dh-tool-catalog-reconcile-2026-05-01.md`.
//!
//! ## Policy
//!
//! 1. **Gateway tools are canonical.** Their names are reserved; collisions
//!    resolve in favour of the gateway implementation.
//! 2. **Duplicates within either catalog are rejected.** A harness that
//!    submits two tools with the same name could otherwise hide one behind
//!    the other and bypass policy. A gateway catalog with duplicates is a
//!    configuration bug.
//! 3. **Collisions** — if a harness tool name matches a gateway canonical
//!    name *or* one of its declared aliases, the harness internal is
//!    suppressed and routed through SQ/EQ as the canonical:
//!    - `name_map[harness_name] = canonical_name` — the harness rewrites its
//!      outbound tool calls to the canonical name before emitting them on
//!      the SQ.
//!    - `suppressed_internals` lists every harness name whose internal
//!      implementation must not be invoked locally.
//!    - `effective_tools` advertises the gateway *canonical* schema (not the
//!      harness's possibly-incompatible variant) so the model sees the
//!      argument shape the gateway will actually validate.
//! 4. **Non-colliding harness tools pass through unchanged.**
//! 5. **Effective-tool ordering is deterministic**: gateway canonical tools
//!    in declaration order, then non-colliding harness tools in declaration
//!    order. Fixtures depend on this.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A tool declared by a BYOH harness at registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessToolDecl {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
}

/// A BYOH harness's full submitted tool catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCatalog {
    pub harness_id: String,
    pub tools: Vec<HarnessToolDecl>,
}

/// A canonical gateway-managed tool. `aliases` lists other names a BYOH
/// harness might use for the same purpose; any matching harness tool will be
/// rewritten to `name`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayToolDecl {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub description: String,
    pub schema: serde_json::Value,
}

/// Output of [`reconcile_byoh_catalog`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciledCatalog {
    /// The catalog the harness should advertise to the model: gateway
    /// canonical tools first, then non-colliding harness tools.
    pub effective_tools: Vec<HarnessToolDecl>,
    /// `harness_name → canonical_name`. The harness MUST rewrite outbound
    /// tool calls bearing `harness_name` to `canonical_name` before emitting
    /// them on SQ/EQ.
    pub name_map: HashMap<String, String>,
    /// Tools the harness MUST NOT execute through its own internal
    /// dispatcher. Every entry corresponds to a collision — either a direct
    /// canonical-name match or a gateway alias match.
    pub suppressed_internals: HashSet<String>,
}

impl ReconciledCatalog {
    /// Resolve a harness-emitted tool call name to its canonical name.
    /// Returns the input unchanged when no rewrite is needed.
    pub fn resolve_call<'a>(&'a self, harness_name: &'a str) -> &'a str {
        self.name_map
            .get(harness_name)
            .map(String::as_str)
            .unwrap_or(harness_name)
    }

    /// True if the harness must suppress its own internal implementation of
    /// `name` and route through SQ/EQ.
    pub fn is_suppressed_internal(&self, name: &str) -> bool {
        self.suppressed_internals.contains(name)
    }

    /// True if `name` appears in `effective_tools`.
    pub fn advertises(&self, name: &str) -> bool {
        self.effective_tools.iter().any(|t| t.name == name)
    }
}

/// Reconciliation failure modes. All are registration-time rejections; the
/// gateway must refuse the harness rather than admit it with an ambiguous
/// catalog.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ReconcileError {
    /// The harness submitted two tool decls with the same name. Capability
    /// laundering: one shadow could be invoked while the other is policed.
    #[error("harness catalog declares tool name twice: {name}")]
    DuplicateHarnessTool { name: String },

    /// The gateway catalog has duplicate canonical names or alias-vs-alias
    /// conflicts across different canonicals — a configuration bug.
    #[error("gateway catalog declares tool name twice: {name}")]
    DuplicateGatewayTool { name: String },

    /// A gateway tool declares an alias equal to a different gateway tool's
    /// canonical name. Ambiguous: which canonical wins for that alias?
    #[error(
        "gateway tool '{tool}' has alias '{alias}' that collides with another gateway canonical name"
    )]
    GatewayAliasCollidesWithCanonical { tool: String, alias: String },
}

/// Reconcile a harness-submitted tool catalog against the gateway's
/// canonical catalog. See module docs for the policy.
pub fn reconcile_byoh_catalog(
    harness: &HarnessCatalog,
    gateway: &[GatewayToolDecl],
) -> Result<ReconciledCatalog, ReconcileError> {
    let mut canonical_set: HashSet<&str> = HashSet::new();
    for tool in gateway {
        if !canonical_set.insert(tool.name.as_str()) {
            return Err(ReconcileError::DuplicateGatewayTool {
                name: tool.name.clone(),
            });
        }
    }

    let mut alias_to_canonical: HashMap<&str, &str> = HashMap::new();
    for tool in gateway {
        for alias in &tool.aliases {
            if alias == &tool.name {
                continue;
            }
            if canonical_set.contains(alias.as_str()) {
                return Err(ReconcileError::GatewayAliasCollidesWithCanonical {
                    tool: tool.name.clone(),
                    alias: alias.clone(),
                });
            }
            if let Some(prior) = alias_to_canonical.get(alias.as_str())
                && *prior != tool.name.as_str()
            {
                return Err(ReconcileError::DuplicateGatewayTool {
                    name: alias.clone(),
                });
            }
            alias_to_canonical.insert(alias.as_str(), tool.name.as_str());
        }
    }

    let mut harness_seen: HashSet<&str> = HashSet::new();
    for tool in &harness.tools {
        if !harness_seen.insert(tool.name.as_str()) {
            return Err(ReconcileError::DuplicateHarnessTool {
                name: tool.name.clone(),
            });
        }
    }

    let mut name_map: HashMap<String, String> = HashMap::new();
    let mut suppressed: HashSet<String> = HashSet::new();
    let mut effective: Vec<HarnessToolDecl> = Vec::with_capacity(gateway.len() + harness.tools.len());

    for tool in gateway {
        effective.push(HarnessToolDecl {
            name: tool.name.clone(),
            description: tool.description.clone(),
            schema: tool.schema.clone(),
        });
    }

    for tool in &harness.tools {
        let canonical_match = if canonical_set.contains(tool.name.as_str()) {
            Some(tool.name.as_str())
        } else {
            alias_to_canonical.get(tool.name.as_str()).copied()
        };

        match canonical_match {
            Some(canonical) => {
                suppressed.insert(tool.name.clone());
                if tool.name != canonical {
                    name_map.insert(tool.name.clone(), canonical.to_string());
                }
            }
            None => {
                effective.push(tool.clone());
            }
        }
    }

    Ok(ReconciledCatalog {
        effective_tools: effective,
        name_map,
        suppressed_internals: suppressed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn h(name: &str) -> HarnessToolDecl {
        HarnessToolDecl {
            name: name.to_string(),
            description: format!("harness {name}"),
            schema: json!({"type": "object"}),
        }
    }

    fn g(name: &str, aliases: &[&str]) -> GatewayToolDecl {
        GatewayToolDecl {
            name: name.to_string(),
            aliases: aliases.iter().map(|s| s.to_string()).collect(),
            description: format!("gateway {name}"),
            schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }
    }

    #[test]
    fn empty_catalogs_produce_empty_reconciliation() {
        let catalog = HarnessCatalog {
            harness_id: "h1".into(),
            tools: vec![],
        };
        let reconciled = reconcile_byoh_catalog(&catalog, &[]).unwrap();
        assert!(reconciled.effective_tools.is_empty());
        assert!(reconciled.name_map.is_empty());
        assert!(reconciled.suppressed_internals.is_empty());
    }

    #[test]
    fn non_colliding_harness_tools_pass_through() {
        let catalog = HarnessCatalog {
            harness_id: "h1".into(),
            tools: vec![h("Bash"), h("Search")],
        };
        let gateway = vec![g("read_file", &[])];
        let reconciled = reconcile_byoh_catalog(&catalog, &gateway).unwrap();

        let names: Vec<&str> = reconciled
            .effective_tools
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, vec!["read_file", "Bash", "Search"]);
        assert!(reconciled.name_map.is_empty());
        assert!(reconciled.suppressed_internals.is_empty());
        assert_eq!(reconciled.resolve_call("Bash"), "Bash");
    }

    /// sera-w2dh acceptance criterion §3 — poison-tool probe.
    ///
    /// A harness with internal `Read` plus a gateway with `read_file`
    /// (alias `Read`) yields exactly one canonical invocation through
    /// gateway dispatch; the harness's internal `Read` is provably
    /// bypassed: it does not appear in the advertised catalog and it is
    /// listed in `suppressed_internals`.
    #[test]
    fn poison_tool_probe_read_alias_routes_to_gateway() {
        let catalog = HarnessCatalog {
            harness_id: "claude-code".into(),
            tools: vec![h("Read"), h("Bash")],
        };
        let gateway = vec![g("read_file", &["Read"])];
        let reconciled = reconcile_byoh_catalog(&catalog, &gateway).unwrap();

        assert!(!reconciled.advertises("Read"));
        assert!(reconciled.advertises("read_file"));
        assert!(reconciled.advertises("Bash"));

        let read_file = reconciled
            .effective_tools
            .iter()
            .find(|t| t.name == "read_file")
            .expect("gateway canonical advertised");
        assert!(
            read_file.description.starts_with("gateway"),
            "description must come from gateway tool, got {:?}",
            read_file.description
        );
        assert!(
            read_file.schema["properties"]["path"].is_object(),
            "schema must be the gateway schema; got {:?}",
            read_file.schema
        );

        assert!(reconciled.is_suppressed_internal("Read"));
        assert_eq!(
            reconciled.name_map.get("Read"),
            Some(&"read_file".to_string())
        );

        // Dispatch property: a tool_call with name="Read" resolves to the
        // canonical exactly once. There is no fork because the harness's
        // own `Read` is not in `effective_tools` and is in `suppressed_internals`.
        assert_eq!(reconciled.resolve_call("Read"), "read_file");

        assert!(!reconciled.is_suppressed_internal("Bash"));
        assert!(!reconciled.name_map.contains_key("Bash"));
    }

    /// Capability laundering blocker (sera-w2dh §4): a harness that submits
    /// two tools sharing a name is rejected outright.
    #[test]
    fn duplicate_harness_tool_name_is_rejected() {
        let catalog = HarnessCatalog {
            harness_id: "h1".into(),
            tools: vec![h("Read"), h("Read")],
        };
        let err = reconcile_byoh_catalog(&catalog, &[]).unwrap_err();
        assert_eq!(
            err,
            ReconcileError::DuplicateHarnessTool {
                name: "Read".into()
            }
        );
    }

    #[test]
    fn duplicate_gateway_tool_name_is_rejected() {
        let catalog = HarnessCatalog {
            harness_id: "h1".into(),
            tools: vec![],
        };
        let gateway = vec![g("read_file", &[]), g("read_file", &[])];
        let err = reconcile_byoh_catalog(&catalog, &gateway).unwrap_err();
        assert_eq!(
            err,
            ReconcileError::DuplicateGatewayTool {
                name: "read_file".into()
            }
        );
    }

    /// Exact-name collision (no alias) still suppresses the harness internal
    /// and forces the gateway schema. Without this, a harness could submit
    /// `read_file` with its own implementation and bypass AuthZ.
    #[test]
    fn harness_tool_matching_canonical_name_is_suppressed() {
        let catalog = HarnessCatalog {
            harness_id: "h1".into(),
            tools: vec![h("read_file")],
        };
        let gateway = vec![g("read_file", &[])];
        let reconciled = reconcile_byoh_catalog(&catalog, &gateway).unwrap();

        assert!(reconciled.is_suppressed_internal("read_file"));
        // Same canonical name → no rewrite needed in the name map.
        assert!(!reconciled.name_map.contains_key("read_file"));
        let read_file = reconciled
            .effective_tools
            .iter()
            .find(|t| t.name == "read_file")
            .expect("gateway canonical wins");
        assert!(read_file.description.starts_with("gateway"));
    }

    #[test]
    fn gateway_alias_colliding_with_other_canonical_is_rejected() {
        let catalog = HarnessCatalog {
            harness_id: "h1".into(),
            tools: vec![],
        };
        let gateway = vec![g("read_file", &["write_file"]), g("write_file", &[])];
        let err = reconcile_byoh_catalog(&catalog, &gateway).unwrap_err();
        assert!(matches!(
            err,
            ReconcileError::GatewayAliasCollidesWithCanonical { .. }
        ));
    }

    #[test]
    fn alias_equal_to_own_canonical_is_ignored() {
        let catalog = HarnessCatalog {
            harness_id: "h1".into(),
            tools: vec![h("Bash")],
        };
        let gateway = vec![g("read_file", &["read_file"])];
        let reconciled = reconcile_byoh_catalog(&catalog, &gateway).unwrap();
        let names: Vec<&str> = reconciled
            .effective_tools
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(names, vec!["read_file", "Bash"]);
    }

    #[test]
    fn deterministic_ordering_gateway_first_then_harness() {
        let catalog = HarnessCatalog {
            harness_id: "h1".into(),
            tools: vec![h("Z_tool"), h("A_tool")],
        };
        let gateway = vec![g("z_canonical", &[]), g("a_canonical", &[])];
        let reconciled = reconcile_byoh_catalog(&catalog, &gateway).unwrap();
        let names: Vec<&str> = reconciled
            .effective_tools
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["z_canonical", "a_canonical", "Z_tool", "A_tool"]
        );
    }

    /// Two gateway tools cannot share an alias — that would make the alias
    /// rewrite ambiguous.
    #[test]
    fn duplicate_alias_across_gateway_tools_is_rejected() {
        let catalog = HarnessCatalog {
            harness_id: "h1".into(),
            tools: vec![],
        };
        let gateway = vec![g("read_file", &["File"]), g("read_chunk", &["File"])];
        let err = reconcile_byoh_catalog(&catalog, &gateway).unwrap_err();
        assert_eq!(
            err,
            ReconcileError::DuplicateGatewayTool {
                name: "File".into()
            }
        );
    }

    #[test]
    fn reconciled_catalog_serde_roundtrip() {
        let catalog = HarnessCatalog {
            harness_id: "h1".into(),
            tools: vec![h("Read"), h("Bash")],
        };
        let gateway = vec![g("read_file", &["Read"])];
        let reconciled = reconcile_byoh_catalog(&catalog, &gateway).unwrap();
        let json = serde_json::to_string(&reconciled).unwrap();
        let parsed: ReconciledCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, reconciled);
    }
}
