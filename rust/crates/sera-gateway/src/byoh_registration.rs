//! BYOH harness registration handshake (bead sera-w2dh, registration slice).
//!
//! Pattern A only — Pattern B sessions do not share a tool catalog with the
//! gateway, so this module's contract does not apply there.
//!
//! ## What this is
//!
//! [`byoh_catalog`](crate::byoh_catalog) defines the pure reconciliation policy:
//! given a harness's submitted catalog and the gateway's canonical catalog, it
//! produces a [`ReconciledCatalog`] (effective tools, `name_map`, suppressed
//! internals). That module is intentionally a pure function — no registry, no
//! dispatch.
//!
//! This module is the smallest registration *seam* on top of that reconciler:
//!
//! 1. [`ByohCatalogRegistry::register_harness`] — the handshake op a BYOH
//!    harness invokes once at boot. Stores the [`ReconciledCatalog`] keyed by
//!    `harness_id` and returns it so the harness can install it locally.
//! 2. [`ByohCatalogRegistry::prepare_outbound_call`] — the dispatch-decision
//!    seam. Given a harness emitting a tool call by its own name, it returns
//!    [`OutboundCallDecision`] telling the harness whether to (a) suppress its
//!    internal and forward to the gateway as a canonical call, or (b) pass the
//!    call through to its own internal implementation.
//!
//! No HTTP route is added here. That arrives in the next slice (a public op
//! over the existing gateway transport); this seam is what the route will
//! delegate to. Keeping it as a module keeps the unit / integration test
//! footprint narrow and avoids dragging in an `AppState`.
//!
//! ## Why this matters (the poison-tool probe)
//!
//! Without registration, a harness that ships an internal `Read` tool plus a
//! gateway exposing `read_file` (alias `Read`) can produce *capability
//! laundering*: the model emits `Read`, the harness invokes its internal
//! implementation, and the gateway never sees the call — bypassing AuthZ +
//! audit. The reconciler suppresses that path declaratively; this module
//! turns the declaration into an enforced runtime decision.
//!
//! See `tests/byoh_registration_handshake.rs` for the end-to-end probe.

use std::collections::HashMap;
use std::sync::RwLock;

use thiserror::Error;

use crate::byoh_catalog::{
    GatewayToolDecl, HarnessCatalog, ReconcileError, ReconciledCatalog, reconcile_byoh_catalog,
};

/// Registry of reconciled BYOH tool catalogs.
///
/// One instance lives in the gateway. Each registered harness contributes one
/// [`ReconciledCatalog`] keyed by its `harness_id`. The gateway-canonical
/// catalog is supplied at construction and shared across registrations — it
/// does not change at handshake time.
pub struct ByohCatalogRegistry {
    gateway_tools: Vec<GatewayToolDecl>,
    catalogs: RwLock<HashMap<String, ReconciledCatalog>>,
}

/// Errors from registry operations.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum RegistrationError {
    /// The catalog the harness submitted was rejected by the reconciler. Wraps
    /// the upstream error so callers can surface the underlying reason
    /// (duplicate name, ambiguous alias, …) without losing fidelity.
    #[error("reconciliation failed: {0}")]
    Reconcile(#[from] ReconcileError),
}

/// Decision returned by [`ByohCatalogRegistry::prepare_outbound_call`] when a
/// harness wants to act on a model-emitted tool call.
///
/// The variants match the three legitimate outcomes; an error is returned for
/// anything else (unknown harness, unknown tool name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundCallDecision {
    /// The harness MUST suppress its own internal implementation and forward
    /// the call to the gateway under `canonical`. Set when the model emitted a
    /// name that collides directly with a canonical or matches an alias.
    SuppressInternalAndForward { canonical: String },
    /// No collision — the harness invokes its own internal implementation as
    /// usual. The gateway is not involved for this tool.
    PassthroughToInternal,
}

/// Errors from [`ByohCatalogRegistry::prepare_outbound_call`].
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum DispatchError {
    /// No catalog has been registered under this `harness_id`. Either the
    /// harness skipped the handshake or it is sending calls before
    /// registration completed.
    #[error("harness not registered: {harness_id}")]
    HarnessNotRegistered { harness_id: String },

    /// The harness emitted a name that was neither in `effective_tools` nor in
    /// the suppressed/aliased set. The harness violated the contract — its
    /// model should only see tools advertised by the reconciled catalog.
    #[error("harness '{harness_id}' emitted unknown tool name '{name}'")]
    UnknownTool { harness_id: String, name: String },
}

impl ByohCatalogRegistry {
    /// Build a registry seeded with the gateway's canonical catalog.
    pub fn new(gateway_tools: Vec<GatewayToolDecl>) -> Self {
        Self {
            gateway_tools,
            catalogs: RwLock::new(HashMap::new()),
        }
    }

    /// Number of currently registered harnesses. Test introspection only.
    pub fn registered_count(&self) -> usize {
        self.catalogs
            .read()
            .expect("catalogs read lock poisoned")
            .len()
    }

    /// Register a harness's catalog. Reconciles against the gateway canonical
    /// catalog and stores the result keyed by `harness.harness_id`.
    ///
    /// A second call for the same `harness_id` replaces the prior catalog. The
    /// production handshake flow is "register once at boot"; replacement
    /// supports test reconfiguration and any future hot-reload story without
    /// inventing a separate "update" op.
    ///
    /// Returns the [`ReconciledCatalog`] so the caller can serialize it back
    /// over the wire to the harness.
    pub fn register_harness(
        &self,
        harness: HarnessCatalog,
    ) -> Result<ReconciledCatalog, RegistrationError> {
        let reconciled = reconcile_byoh_catalog(&harness, &self.gateway_tools)?;
        let mut guard = self.catalogs.write().expect("catalogs write lock poisoned");
        guard.insert(harness.harness_id.clone(), reconciled.clone());
        Ok(reconciled)
    }

    /// Look up a stored reconciled catalog. Returns `None` if no harness with
    /// `harness_id` has registered.
    pub fn get(&self, harness_id: &str) -> Option<ReconciledCatalog> {
        self.catalogs
            .read()
            .expect("catalogs read lock poisoned")
            .get(harness_id)
            .cloned()
    }

    /// Decide what a registered harness should do with a model-emitted tool
    /// call. The harness MUST honour the returned decision: when a forward is
    /// requested, the harness's own internal implementation must not run.
    pub fn prepare_outbound_call(
        &self,
        harness_id: &str,
        emitted_name: &str,
    ) -> Result<OutboundCallDecision, DispatchError> {
        let guard = self.catalogs.read().expect("catalogs read lock poisoned");
        let reconciled =
            guard
                .get(harness_id)
                .ok_or_else(|| DispatchError::HarnessNotRegistered {
                    harness_id: harness_id.to_string(),
                })?;

        if reconciled.is_suppressed_internal(emitted_name) {
            let canonical = reconciled.resolve_call(emitted_name).to_string();
            return Ok(OutboundCallDecision::SuppressInternalAndForward { canonical });
        }

        if reconciled.advertises(emitted_name) {
            // Names that are in `effective_tools` but not suppressed are
            // either harness pass-throughs or canonicals. A canonical reaches
            // this branch when the harness did not declare a colliding tool of
            // its own; since the gateway tool is advertised under its
            // canonical name, the harness has no internal of that name —
            // forward to the gateway.
            if self
                .gateway_tools
                .iter()
                .any(|g| g.name == emitted_name)
            {
                return Ok(OutboundCallDecision::SuppressInternalAndForward {
                    canonical: emitted_name.to_string(),
                });
            }
            return Ok(OutboundCallDecision::PassthroughToInternal);
        }

        Err(DispatchError::UnknownTool {
            harness_id: harness_id.to_string(),
            name: emitted_name.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::byoh_catalog::HarnessToolDecl;
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
    fn register_harness_returns_reconciled_catalog() {
        let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
        let reconciled = registry
            .register_harness(HarnessCatalog {
                harness_id: "claude-code".into(),
                tools: vec![h("Read"), h("Bash")],
            })
            .expect("registration ok");

        assert!(reconciled.advertises("read_file"));
        assert!(reconciled.advertises("Bash"));
        assert!(!reconciled.advertises("Read"));
        assert!(reconciled.is_suppressed_internal("Read"));
        assert_eq!(
            reconciled.name_map.get("Read"),
            Some(&"read_file".to_string())
        );
    }

    #[test]
    fn register_harness_persists_under_harness_id() {
        let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
        registry
            .register_harness(HarnessCatalog {
                harness_id: "claude-code".into(),
                tools: vec![h("Read")],
            })
            .unwrap();

        assert_eq!(registry.registered_count(), 1);
        let stored = registry.get("claude-code").expect("registered");
        assert!(stored.is_suppressed_internal("Read"));
        assert!(registry.get("not-registered").is_none());
    }

    #[test]
    fn second_registration_replaces_prior_catalog() {
        let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
        registry
            .register_harness(HarnessCatalog {
                harness_id: "claude-code".into(),
                tools: vec![h("Read"), h("OldTool")],
            })
            .unwrap();
        registry
            .register_harness(HarnessCatalog {
                harness_id: "claude-code".into(),
                tools: vec![h("Read")],
            })
            .unwrap();

        assert_eq!(registry.registered_count(), 1);
        let stored = registry.get("claude-code").unwrap();
        assert!(!stored.advertises("OldTool"));
    }

    #[test]
    fn register_propagates_reconcile_error() {
        let registry = ByohCatalogRegistry::new(vec![]);
        let err = registry
            .register_harness(HarnessCatalog {
                harness_id: "h1".into(),
                tools: vec![h("Read"), h("Read")],
            })
            .unwrap_err();
        assert_eq!(
            err,
            RegistrationError::Reconcile(ReconcileError::DuplicateHarnessTool {
                name: "Read".into()
            })
        );
        assert_eq!(registry.registered_count(), 0);
    }

    #[test]
    fn prepare_outbound_call_forwards_aliased_name() {
        let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
        registry
            .register_harness(HarnessCatalog {
                harness_id: "h1".into(),
                tools: vec![h("Read"), h("Bash")],
            })
            .unwrap();

        let decision = registry.prepare_outbound_call("h1", "Read").unwrap();
        assert_eq!(
            decision,
            OutboundCallDecision::SuppressInternalAndForward {
                canonical: "read_file".into()
            }
        );
    }

    #[test]
    fn prepare_outbound_call_forwards_canonical_name_directly() {
        // Even with no harness collision, a model that emits the canonical
        // name (e.g. because the harness advertised it via the reconciled
        // catalog) must reach the gateway, not the harness.
        let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
        registry
            .register_harness(HarnessCatalog {
                harness_id: "h1".into(),
                tools: vec![h("Bash")],
            })
            .unwrap();

        let decision = registry.prepare_outbound_call("h1", "read_file").unwrap();
        assert_eq!(
            decision,
            OutboundCallDecision::SuppressInternalAndForward {
                canonical: "read_file".into()
            }
        );
    }

    #[test]
    fn prepare_outbound_call_passthrough_for_non_colliding_harness_tool() {
        let registry = ByohCatalogRegistry::new(vec![g("read_file", &["Read"])]);
        registry
            .register_harness(HarnessCatalog {
                harness_id: "h1".into(),
                tools: vec![h("Bash")],
            })
            .unwrap();

        let decision = registry.prepare_outbound_call("h1", "Bash").unwrap();
        assert_eq!(decision, OutboundCallDecision::PassthroughToInternal);
    }

    #[test]
    fn prepare_outbound_call_unknown_harness_errors() {
        let registry = ByohCatalogRegistry::new(vec![g("read_file", &[])]);
        let err = registry
            .prepare_outbound_call("ghost", "anything")
            .unwrap_err();
        assert_eq!(
            err,
            DispatchError::HarnessNotRegistered {
                harness_id: "ghost".into()
            }
        );
    }

    #[test]
    fn prepare_outbound_call_unknown_tool_errors() {
        let registry = ByohCatalogRegistry::new(vec![g("read_file", &[])]);
        registry
            .register_harness(HarnessCatalog {
                harness_id: "h1".into(),
                tools: vec![h("Bash")],
            })
            .unwrap();

        let err = registry
            .prepare_outbound_call("h1", "Mystery")
            .unwrap_err();
        assert_eq!(
            err,
            DispatchError::UnknownTool {
                harness_id: "h1".into(),
                name: "Mystery".into()
            }
        );
    }
}
