//! HTTP route handlers exposed via the gateway library.
//!
//! Today only [`inference_proxy`] needs library-level visibility (so
//! integration tests under `tests/` can drive it through the production
//! audit/budget seams). Other route modules continue to be loaded directly
//! into the binary via `#[path]`; promote them here as integration tests
//! grow to need them.

pub mod circles;
pub mod inference_proxy;
pub mod evolution;
pub mod subagents;
pub mod intercom;
