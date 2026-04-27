//! Capability policy — gateway-side audit glue (sera-eo71).
//!
//! Pre-dispatch enforcement now lives inside `sera-runtime` so that denied
//! tools never start. The canonical [`CapabilityRegistry`] / [`PolicyDenial`]
//! types are owned by `sera-config` and re-exported here so existing
//! `sera_gateway::capability_enforcement::…` import paths keep working.
//!
//! The gateway still loads the registry at boot (and exposes the
//! `/admin/policies/*` reload routes), but the only runtime role left here is
//! observability: the `enforce_tool_events` filter in `bin/sera.rs` watches
//! NDJSON `tool_call_begin` frames from the runtime and emits OCSF Policy
//! Activity audit entries when the runtime reports a denied call. With the
//! runtime blocking pre-dispatch, those denials arrive as already-formatted
//! `[sera-policy] tool '…' denied …` events; the gateway-side path is
//! defence-in-depth.

pub use sera_config::{
    CapabilityPolicy, CapabilityRegistry, CapabilityRegistryError, PolicyDenial, POLICIES_DIR_ENV,
};
