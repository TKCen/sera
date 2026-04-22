//! Integration smoke test for the `sera:hooks` WIT public contract.
//!
//! The WIT file at `wit/sera-hooks.wit` is a **public contract** — third-party
//! hook authors compile against it with `wit-bindgen`. Breaking the file breaks
//! every published hook. This test pins the shape and catches accidental
//! rename/removal of the exported interfaces, worlds, and capability functions.
//!
//! Scope:
//! - The file parses cleanly under `wit-parser` (i.e. a valid `wit` package).
//! - The `sera-hook` world exports the `hook` interface.
//! - The `sera-hook` world imports `host-capabilities`.
//! - The `host-capabilities` interface declares exactly the four sandboxed
//!   functions (`log`, `state-get`, `state-set`, `emit-audit`).
//! - The `hook` interface declares `init`, `metadata`, `execute`.
//! - All 20 lifecycle points from `sera-types::hook::HookPoint` are present
//!   in the WIT `hook-point` enum under their kebab-case form.
//!
//! Failure modes flagged here catch public-contract drift at `cargo test` time.

use std::path::PathBuf;

use sera_types::hook::HookPoint;
use wit_parser::Resolve;

fn wit_path() -> PathBuf {
    // CARGO_MANIFEST_DIR resolves to the crate root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("wit")
        .join("sera-hooks.wit")
}

fn load() -> Resolve {
    let mut resolve = Resolve::default();
    let _pkg_id = resolve
        .push_file(wit_path())
        .expect("wit/sera-hooks.wit must parse cleanly — it is a public contract");
    resolve
}

#[test]
fn wit_file_parses() {
    let _ = load();
}

#[test]
fn world_sera_hook_exists_and_wires_imports_and_exports() {
    let resolve = load();
    let world_id = resolve
        .worlds
        .iter()
        .find_map(|(id, w)| (w.name == "sera-hook").then_some(id))
        .expect("world `sera-hook` is the public world third parties target");
    let world = &resolve.worlds[world_id];

    // At least one import and exactly one export (`hook`).
    assert!(
        !world.imports.is_empty(),
        "sera-hook must import host-capabilities"
    );
    assert_eq!(
        world.exports.len(),
        1,
        "sera-hook must export exactly the `hook` interface (got {})",
        world.exports.len()
    );
}

#[test]
fn host_capabilities_interface_declares_exactly_four_functions() {
    let resolve = load();
    let iface_id = resolve
        .interfaces
        .iter()
        .find_map(|(id, i)| (i.name.as_deref() == Some("host-capabilities")).then_some(id))
        .expect("host-capabilities interface must exist — it is THE sandbox surface");
    let iface = &resolve.interfaces[iface_id];

    let mut names: Vec<&str> = iface.functions.keys().map(|s| s.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["emit-audit", "log", "state-get", "state-set"],
        "The sandbox surface is part of the security contract; growth is intentional-only"
    );
}

#[test]
fn hook_interface_declares_lifecycle_functions() {
    let resolve = load();
    let iface_id = resolve
        .interfaces
        .iter()
        .find_map(|(id, i)| (i.name.as_deref() == Some("hook")).then_some(id))
        .expect("hook interface must exist");
    let iface = &resolve.interfaces[iface_id];

    let mut names: Vec<&str> = iface.functions.keys().map(|s| s.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["execute", "init", "metadata"],
        "Hook lifecycle mirrors the in-process Hook trait; changes break every hook"
    );
}

#[test]
fn hook_point_enum_covers_all_20_sera_points() {
    let resolve = load();
    let types_iface_id = resolve
        .interfaces
        .iter()
        .find_map(|(id, i)| (i.name.as_deref() == Some("types")).then_some(id))
        .expect("types interface must exist");
    let iface = &resolve.interfaces[types_iface_id];

    let hook_point_ty = iface
        .types
        .get("hook-point")
        .copied()
        .expect("hook-point type must exist in types interface");
    let ty = &resolve.types[hook_point_ty];

    let wit_parser::TypeDefKind::Enum(ref e) = ty.kind else {
        panic!("hook-point must be an enum");
    };

    // WIT identifiers are kebab-case; sera-types' serde form is snake_case.
    // Compare by converting underscores to hyphens.
    let wit_names: std::collections::BTreeSet<String> =
        e.cases.iter().map(|c| c.name.clone()).collect();

    for point in HookPoint::ALL {
        let serde_name = serde_json::to_string(point).unwrap();
        let kebab = serde_name.trim_matches('"').replace('_', "-");
        assert!(
            wit_names.contains(&kebab),
            "hook-point `{kebab}` missing from WIT enum (HookPoint::{point:?}); WIT and HookPoint must stay in sync"
        );
    }

    assert_eq!(
        wit_names.len(),
        HookPoint::ALL.len(),
        "WIT hook-point count ({}) must equal HookPoint::ALL ({})",
        wit_names.len(),
        HookPoint::ALL.len()
    );
}

#[test]
fn hook_result_variant_has_continue_reject_redirect() {
    let resolve = load();
    let types_iface_id = resolve
        .interfaces
        .iter()
        .find_map(|(id, i)| (i.name.as_deref() == Some("types")).then_some(id))
        .expect("types interface must exist");
    let iface = &resolve.interfaces[types_iface_id];

    let hook_result_ty = iface
        .types
        .get("hook-result")
        .copied()
        .expect("hook-result variant must exist");
    let ty = &resolve.types[hook_result_ty];
    let wit_parser::TypeDefKind::Variant(ref v) = ty.kind else {
        panic!("hook-result must be a variant");
    };

    let mut cases: Vec<&str> = v.cases.iter().map(|c| c.name.as_str()).collect();
    cases.sort();
    assert_eq!(
        cases,
        vec!["continue", "redirect", "reject"],
        "hook-result variants mirror HookResult::{{Continue,Reject,Redirect}}"
    );
}
