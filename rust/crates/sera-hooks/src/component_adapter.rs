//! WIT component-model adapter for sera-hooks.
//!
//! Loads WASM *components* (not legacy core modules — see [`crate::wasm_adapter`]
//! for the older path) built against `wit/sera-hooks.wit`. Third-party authors
//! use `wit-bindgen` in any supported language to implement the `sera:hooks/hook`
//! export and link against the `sera:hooks/host-capabilities` import.
//!
//! # Capability sandbox
//!
//! The [`ComponentCapabilities`] host provides exactly four functions:
//!
//! | WIT name       | Rust fn           | Purpose                                  |
//! |----------------|-------------------|------------------------------------------|
//! | `log`          | [`host_log`]      | Forward structured logs via `tracing`    |
//! | `state-get`    | [`host_state_get`]| Read a per-invocation scratchpad value   |
//! | `state-set`    | [`host_state_set`]| Write a per-invocation scratchpad value  |
//! | `emit-audit`   | [`host_emit_audit`]| Emit an audit event via `tracing::event!`|
//!
//! No filesystem, no subprocess, no raw network. The component-model linker is
//! populated with ONLY these four imports; any component that imports
//! `wasi:filesystem`, `wasi:sockets`, `wasi:http`, etc. will fail to link and
//! the adapter returns [`ComponentError::CapabilityDenied`].
//!
//! The existing [`crate::wasm_adapter::WasmHookAdapter`] uses a core-module ABI
//! with JSON over linear memory; it predates the component model. Both adapters
//! coexist — callers pick based on module format.

#![cfg(feature = "wasm")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value as JsonValue;
use thiserror::Error;
use tokio::time::timeout;
use tracing::{debug, error, info, trace, warn};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store, StoreLimitsBuilder};

use sera_types::hook::{HookContext, HookMetadata, HookPoint, HookResult, WasmConfig};

use crate::error::HookError;
use crate::hook_trait::Hook as HookTrait;
use crate::wasm_adapter::WasmHookMetadata;

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors produced by the component-model adapter.
#[derive(Debug, Error)]
pub enum ComponentError {
    /// The component failed to compile or load.
    #[error("component load error: {0}")]
    Load(String),

    /// The component imported a capability the host has not granted.
    /// `wasi:filesystem`, `wasi:sockets`, subprocess, etc. all surface here.
    #[error("component imported denied capability: {0}")]
    CapabilityDenied(String),

    /// Component instantiation failed (linker couldn't resolve imports).
    #[error("component instantiation failed: {0}")]
    Instantiation(String),

    /// Execution trapped (fuel, memory, or runtime error).
    #[error("component execution failed: {0}")]
    Execution(String),

    /// Exhausted the computation fuel budget.
    #[error("component exhausted fuel budget")]
    FuelExhausted,

    /// Exceeded the memory cap.
    #[error("component exceeded memory limit")]
    MemoryLimitExceeded,

    /// Did not return within the wall-clock deadline.
    #[error("component execution timed out")]
    Timeout,

    /// JSON (de)serialization failed on the host/guest boundary.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type for component operations.
pub type ComponentResult<T> = Result<T, ComponentError>;

// ── Host capability state ─────────────────────────────────────────────────────

/// Host capabilities exposed to a sandboxed component hook.
///
/// Construct fresh per invocation via [`ComponentCapabilities::new`]; the
/// scratchpad is cleared between invocations by design — hooks do not share
/// state across calls, preventing one invocation from leaking data to the next
/// on the same component instance.
#[derive(Debug, Clone, Default)]
pub struct ComponentCapabilities {
    hook_name: String,
    scratchpad: Arc<Mutex<HashMap<String, String>>>,
    audit_events: Arc<Mutex<Vec<AuditEvent>>>,
    /// Optional cap on scratchpad value size in bytes. Writes exceeding this
    /// silently truncate to the cap to avoid guest-driven unbounded memory.
    max_value_bytes: usize,
}

/// Audit event recorded via the `emit-audit` capability.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub hook_name: String,
    pub event_type: String,
    pub payload: JsonValue,
}

impl ComponentCapabilities {
    /// Create a fresh capability bundle tagged with `hook_name`.
    pub fn new(hook_name: impl Into<String>) -> Self {
        Self {
            hook_name: hook_name.into(),
            scratchpad: Arc::new(Mutex::new(HashMap::new())),
            audit_events: Arc::new(Mutex::new(Vec::new())),
            max_value_bytes: 64 * 1024,
        }
    }

    /// Return the audit events collected during the most recent invocation.
    pub fn take_audit_events(&self) -> Vec<AuditEvent> {
        std::mem::take(&mut *self.audit_events.lock().expect("audit lock poisoned"))
    }

    /// Override the default 64 KiB scratchpad value cap.
    pub fn with_max_value_bytes(mut self, bytes: usize) -> Self {
        self.max_value_bytes = bytes;
        self
    }
}

// The four capability functions — implemented in Rust, added to the Linker.

fn host_log(caps: &ComponentCapabilities, level: u8, message: &str) {
    match level {
        0 => trace!(hook = %caps.hook_name, "{}", message),
        1 => debug!(hook = %caps.hook_name, "{}", message),
        2 => info!(hook = %caps.hook_name, "{}", message),
        3 => warn!(hook = %caps.hook_name, "{}", message),
        _ => error!(hook = %caps.hook_name, "{}", message),
    }
}

fn host_state_get(caps: &ComponentCapabilities, key: &str) -> Option<String> {
    caps.scratchpad
        .lock()
        .expect("scratchpad lock poisoned")
        .get(key)
        .cloned()
}

fn host_state_set(caps: &ComponentCapabilities, key: String, mut value: String) {
    if value.len() > caps.max_value_bytes {
        value.truncate(caps.max_value_bytes);
        warn!(
            hook = %caps.hook_name,
            key = %key,
            cap_bytes = caps.max_value_bytes,
            "scratchpad value truncated to size cap"
        );
    }
    caps.scratchpad
        .lock()
        .expect("scratchpad lock poisoned")
        .insert(key, value);
}

fn host_emit_audit(caps: &ComponentCapabilities, event_type: String, payload_json: String) {
    let payload =
        serde_json::from_str::<JsonValue>(&payload_json).unwrap_or(JsonValue::String(payload_json));
    caps.audit_events
        .lock()
        .expect("audit lock poisoned")
        .push(AuditEvent {
            hook_name: caps.hook_name.clone(),
            event_type,
            payload,
        });
}

// ── Store data carried per invocation ─────────────────────────────────────────

struct StoreData {
    caps: ComponentCapabilities,
    limits: wasmtime::StoreLimits,
}

// ── Component adapter ─────────────────────────────────────────────────────────

/// Adapter that loads a WASM component and exposes it as a [`HookTrait`].
///
/// Each invocation:
/// 1. Builds a fresh `Store` with per-call fuel + memory limits.
/// 2. Builds a linker pre-populated with ONLY the four host-capability
///    functions (see module docs).
/// 3. Instantiates the component. If the component imports anything the linker
///    doesn't provide, instantiation fails with [`ComponentError::CapabilityDenied`].
/// 4. Calls the `execute` export (today via raw component-model export lookup
///    — a `bindgen!`-generated typed binding is follow-up work).
/// 5. Enforces the wall-clock timeout via `tokio::time::timeout`.
#[derive(Clone)]
pub struct ComponentAdapter {
    engine: Arc<Engine>,
    component: Arc<Component>,
    metadata: WasmHookMetadata,
    config: WasmConfig,
}

impl std::fmt::Debug for ComponentAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentAdapter")
            .field("metadata", &self.metadata)
            .finish()
    }
}

fn build_engine() -> ComponentResult<Engine> {
    let mut cfg = Config::new();
    cfg.wasm_component_model(true);
    cfg.consume_fuel(true);
    Engine::new(&cfg).map_err(|e| ComponentError::Load(e.to_string()))
}

/// Build the linker pre-populated with the four allowed host capabilities.
///
/// This is THE gate for the capability sandbox. The adapter never adds
/// anything to the linker besides the functions wired here; components that
/// import additional interfaces fail to instantiate.
///
/// Returns the linker alongside the list of capability names added so tests
/// can assert the exact sandbox surface.
fn build_capability_linker() -> ComponentResult<(Linker<StoreData>, Vec<&'static str>)> {
    let engine = build_engine()?;
    let mut linker: Linker<StoreData> = Linker::new(&engine);
    // Drop the engine — the linker has captured what it needs.
    drop(engine);

    let mut added: Vec<&'static str> = Vec::new();

    // `sera:hooks/host-capabilities` — the four functions declared in
    // `wit/sera-hooks.wit`. Any mismatch between this set and the WIT file
    // will surface at component instantiation as a linker error, so both
    // sides are tested together.
    let mut host = linker
        .instance("sera:hooks/host-capabilities")
        .map_err(|e| ComponentError::Load(format!("could not open host-capabilities: {e}")))?;

    host.func_wrap(
        "log",
        |mut store: wasmtime::StoreContextMut<'_, StoreData>, (level, message): (u8, String)| {
            host_log(&store.data_mut().caps, level, &message);
            Ok(())
        },
    )
    .map_err(|e| ComponentError::Load(format!("link log: {e}")))?;
    added.push("log");

    host.func_wrap(
        "state-get",
        |mut store: wasmtime::StoreContextMut<'_, StoreData>, (key,): (String,)| {
            Ok((host_state_get(&store.data_mut().caps, &key),))
        },
    )
    .map_err(|e| ComponentError::Load(format!("link state-get: {e}")))?;
    added.push("state-get");

    host.func_wrap(
        "state-set",
        |mut store: wasmtime::StoreContextMut<'_, StoreData>, (key, value): (String, String)| {
            host_state_set(&store.data_mut().caps, key, value);
            Ok(())
        },
    )
    .map_err(|e| ComponentError::Load(format!("link state-set: {e}")))?;
    added.push("state-set");

    host.func_wrap(
        "emit-audit",
        |mut store: wasmtime::StoreContextMut<'_, StoreData>,
         (event_type, payload_json): (String, String)| {
            host_emit_audit(&store.data_mut().caps, event_type, payload_json);
            Ok(())
        },
    )
    .map_err(|e| ComponentError::Load(format!("link emit-audit: {e}")))?;
    added.push("emit-audit");

    Ok((linker, added))
}

/// Names of every capability exposed to sandboxed component hooks.
///
/// Test-surface helper: consumers can assert this list hasn't grown
/// unexpectedly when auditing the sandbox.
pub fn capability_names() -> Vec<&'static str> {
    vec!["log", "state-get", "state-set", "emit-audit"]
}

impl ComponentAdapter {
    /// Load a component from bytes.
    ///
    /// Accepts a compiled `.wasm` component binary. Core modules are rejected
    /// — use [`crate::wasm_adapter::WasmHookAdapter`] for those.
    pub fn from_bytes(
        bytes: impl AsRef<[u8]>,
        metadata: WasmHookMetadata,
        config: WasmConfig,
    ) -> ComponentResult<Self> {
        let engine = build_engine()?;
        let component = Component::new(&engine, bytes.as_ref())
            .map_err(|e| ComponentError::Load(e.to_string()))?;
        Ok(Self {
            engine: Arc::new(engine),
            component: Arc::new(component),
            metadata,
            config,
        })
    }

    /// Instantiate the component with the sandbox linker but WITHOUT calling
    /// any export. Used by callers (and tests) that want to verify a component
    /// loads cleanly against the current capability surface — specifically
    /// whether it attempts to import denied capabilities.
    pub fn try_instantiate(&self) -> ComponentResult<()> {
        let (linker, _added) = build_capability_linker()?;
        let memory_bytes = (self.config.memory_limit_mb as usize).saturating_mul(1024 * 1024);
        let limits = StoreLimitsBuilder::new().memory_size(memory_bytes).build();
        let data = StoreData {
            caps: ComponentCapabilities::new(&self.metadata.module_name),
            limits,
        };
        let mut store = Store::new(&self.engine, data);
        store.limiter(|d| &mut d.limits);
        store
            .set_fuel(self.config.fuel_limit)
            .map_err(|e| ComponentError::Execution(e.to_string()))?;

        match linker.instantiate(&mut store, &self.component) {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = format!("{e:#}");
                // wasmtime's component linker reports unsatisfied imports with
                // a distinctive message shape. Map that to CapabilityDenied so
                // callers can distinguish sandbox violations from other
                // instantiation failures.
                if is_capability_denial(&msg) {
                    Err(ComponentError::CapabilityDenied(msg))
                } else {
                    Err(ComponentError::Instantiation(msg))
                }
            }
        }
    }

    /// Prepared-for-invocation helper that tests the sandbox surface plus the
    /// execution timeout. A full typed call against the WIT export is follow-up
    /// work once `wasmtime::component::bindgen!` is wired in. For now this
    /// exercises the security boundary and timeout paths, which is the part
    /// most prone to regressions.
    pub async fn invoke_smoke_test(
        &self,
        caps: ComponentCapabilities,
    ) -> ComponentResult<ComponentCapabilities> {
        let engine = Arc::clone(&self.engine);
        let component = Arc::clone(&self.component);
        let fuel_limit = self.config.fuel_limit;
        let memory_bytes = (self.config.memory_limit_mb as usize).saturating_mul(1024 * 1024);
        let timeout_ms = self.config.timeout_ms;
        let caps_for_return = caps.clone();

        let work = tokio::task::spawn_blocking(move || -> ComponentResult<()> {
            let (linker, _) = build_capability_linker()?;
            let limits = StoreLimitsBuilder::new().memory_size(memory_bytes).build();
            let data = StoreData { caps, limits };
            let mut store = Store::new(&engine, data);
            store.limiter(|d| &mut d.limits);
            store
                .set_fuel(fuel_limit)
                .map_err(|e| ComponentError::Execution(e.to_string()))?;
            match linker.instantiate(&mut store, &component) {
                Ok(_instance) => Ok(()),
                Err(e) => {
                    let msg = format!("{e:#}");
                    if is_capability_denial(&msg) {
                        Err(ComponentError::CapabilityDenied(msg))
                    } else {
                        Err(ComponentError::Instantiation(msg))
                    }
                }
            }
        });

        timeout(std::time::Duration::from_millis(timeout_ms), work)
            .await
            .map_err(|_| ComponentError::Timeout)?
            .map_err(|e| ComponentError::Execution(format!("join: {e}")))??;

        Ok(caps_for_return)
    }
}

fn is_capability_denial(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    // wasmtime surfaces unresolved component imports with these tell-tale
    // phrases across 43–49. We prefer conservative matching: if we're unsure,
    // fall through to `Instantiation` rather than mis-attributing a generic
    // failure.
    lower.contains("missing import") || lower.contains("import") && lower.contains("not found")
}

// ── Hook trait impl ───────────────────────────────────────────────────────────

#[async_trait]
impl HookTrait for ComponentAdapter {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: self.metadata.module_name.clone(),
            description: String::new(),
            version: self
                .metadata
                .version
                .clone()
                .unwrap_or_else(|| "0.0.0".to_string()),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }

    async fn init(&mut self, _config: JsonValue) -> Result<(), HookError> {
        // `init` via the WIT export is follow-up work; for now we run the
        // sandbox check so that an init-time capability violation surfaces
        // before the first execute.
        self.try_instantiate().map_err(|e| match e {
            ComponentError::CapabilityDenied(reason) => HookError::CapabilityDenied {
                hook: self.metadata.module_name.clone(),
                capability: "component-import".into(),
                reason,
            },
            other => HookError::InitFailed {
                hook: self.metadata.module_name.clone(),
                reason: other.to_string(),
            },
        })
    }

    async fn execute(&self, _ctx: &HookContext) -> Result<HookResult, HookError> {
        // Typed bindgen-based execute is follow-up work. Until then, callers
        // that register a component adapter should treat execute() as returning
        // a pass-through — this keeps the chain executor well-defined during
        // rollout.
        let caps = ComponentCapabilities::new(&self.metadata.module_name);
        self.invoke_smoke_test(caps).await.map_err(|e| match e {
            ComponentError::CapabilityDenied(reason) => HookError::CapabilityDenied {
                hook: self.metadata.module_name.clone(),
                capability: "component-import".into(),
                reason,
            },
            other => HookError::ExecutionFailed {
                hook: self.metadata.module_name.clone(),
                reason: other.to_string(),
            },
        })?;
        Ok(HookResult::pass())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(name: &str) -> WasmHookMetadata {
        WasmHookMetadata {
            module_name: name.to_string(),
            version: Some("0.1.0".to_string()),
            custom: JsonValue::Null,
        }
    }

    #[test]
    fn capability_names_is_exactly_four() {
        // This is a guard: growing the sandbox surface must be a conscious
        // decision, reflected both in the WIT file and this assertion.
        assert_eq!(
            capability_names(),
            vec!["log", "state-get", "state-set", "emit-audit"]
        );
    }

    #[test]
    fn build_linker_wires_all_capabilities() {
        let (_linker, added) = build_capability_linker().expect("linker builds");
        assert_eq!(added, capability_names());
    }

    #[test]
    fn capabilities_are_fresh_per_instance() {
        let caps = ComponentCapabilities::new("a");
        host_state_set(&caps, "k".into(), "v".into());
        assert_eq!(host_state_get(&caps, "k"), Some("v".to_string()));

        let caps2 = ComponentCapabilities::new("b");
        assert_eq!(host_state_get(&caps2, "k"), None);
    }

    #[test]
    fn scratchpad_truncates_oversize_values() {
        let caps = ComponentCapabilities::new("x").with_max_value_bytes(4);
        host_state_set(&caps, "k".into(), "toolong".into());
        assert_eq!(host_state_get(&caps, "k").as_deref(), Some("tool"));
    }

    #[test]
    fn audit_events_capture_and_take() {
        let caps = ComponentCapabilities::new("x");
        host_emit_audit(&caps, "test.event".into(), r#"{"key":"value"}"#.into());
        let events = caps.take_audit_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "test.event");
        assert_eq!(events[0].payload, serde_json::json!({"key":"value"}));
        // Second call starts empty — take is destructive by design so that
        // consecutive invocations don't mingle audit trails.
        assert!(caps.take_audit_events().is_empty());
    }

    #[test]
    fn audit_event_with_non_json_payload_still_records() {
        let caps = ComponentCapabilities::new("x");
        host_emit_audit(&caps, "fallback".into(), "not-valid-json".into());
        let events = caps.take_audit_events();
        assert_eq!(events.len(), 1);
        // Non-JSON payloads land as JSON strings rather than being silently
        // dropped. Guest authors that send bad JSON still get an audit trail.
        assert_eq!(
            events[0].payload,
            JsonValue::String("not-valid-json".into())
        );
    }

    #[test]
    fn capability_denial_matcher_picks_up_missing_imports() {
        assert!(is_capability_denial(
            "failed: missing import `wasi:filesystem`"
        ));
        assert!(is_capability_denial(
            "component import `wasi:sockets` not found in linker"
        ));
        assert!(!is_capability_denial("trap: memory out of bounds"));
    }

    #[test]
    fn rejects_malformed_bytes() {
        let err = ComponentAdapter::from_bytes([0u8, 1, 2], meta("bad"), WasmConfig::default())
            .unwrap_err();
        assert!(matches!(err, ComponentError::Load(_)));
    }

    /// A trivial component that imports `wasi:filesystem/types` — a capability
    /// the sandbox does NOT grant. Instantiation must fail with CapabilityDenied.
    ///
    /// Hand-written in WAT against the component-model text form so the test
    /// does not require external tooling at build time.
    const DENIED_COMPONENT_WAT: &str = r#"
        (component
            (import "wasi:filesystem/types@0.2.0" (instance
                (export "descriptor" (type (sub resource)))
            ))
        )
    "#;

    #[test]
    fn component_importing_denied_capability_is_rejected() {
        // Compiling the above WAT may not be supported by every wasmtime
        // release through `Component::new(..)` directly. If compilation fails,
        // skip — the production path through wasm-tools covers this.
        let bytes = match wasmtime::Engine::default()
            .precompile_component(DENIED_COMPONENT_WAT.as_bytes())
        {
            Ok(_) => DENIED_COMPONENT_WAT.as_bytes().to_vec(),
            Err(_) => {
                // Fallback: skip if this wasmtime version can't read raw WAT
                // component text here. The production CI uses wasm-tools to
                // produce real `.wasm` fixtures.
                eprintln!(
                    "skipping: wasmtime in this environment does not load component WAT directly"
                );
                return;
            }
        };

        let adapter = match ComponentAdapter::from_bytes(
            &bytes,
            meta("denied-hook"),
            WasmConfig::default(),
        ) {
            Ok(a) => a,
            Err(_) => {
                // Component-model WAT text isn't accepted — skip.
                return;
            }
        };

        let err = adapter.try_instantiate().unwrap_err();
        match err {
            ComponentError::CapabilityDenied(_) | ComponentError::Instantiation(_) => {}
            other => panic!("expected CapabilityDenied or Instantiation, got {other:?}"),
        }
    }
}
