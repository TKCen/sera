//! Integration tests for sera-commands registry and built-in commands.

use sera_commands::{
    CommandArgs, CommandCategory, CommandContext, CommandError, CommandRegistry, PingCommand,
    VersionCommand,
};

fn make_registry() -> CommandRegistry {
    let mut r = CommandRegistry::new();
    r.register(PingCommand);
    r.register(VersionCommand);
    r
}

// --- Registry: register + get + list ---

#[test]
fn registry_get_registered_command() {
    let r = make_registry();
    assert!(r.get("ping").is_some());
    assert!(r.get("version").is_some());
}

#[test]
fn registry_get_unknown_returns_none() {
    let r = make_registry();
    assert!(r.get("does-not-exist").is_none());
}

#[test]
fn registry_list_returns_all() {
    let r = make_registry();
    assert_eq!(r.list().len(), 2);
}

#[test]
fn registry_len_and_is_empty() {
    let mut r = CommandRegistry::new();
    assert!(r.is_empty());
    r.register(PingCommand);
    assert_eq!(r.len(), 1);
    assert!(!r.is_empty());
}

// --- Registry: list_by_category ---

#[test]
fn registry_list_by_category_diagnostic() {
    let r = make_registry();
    let diag = r.list_by_category(CommandCategory::Diagnostic);
    assert_eq!(diag.len(), 1);
    assert_eq!(diag[0].name(), "ping");
}

#[test]
fn registry_list_by_category_system() {
    let r = make_registry();
    let sys = r.list_by_category(CommandCategory::System);
    assert_eq!(sys.len(), 1);
    assert_eq!(sys[0].name(), "version");
}

#[test]
fn registry_list_by_category_empty_when_no_match() {
    let r = make_registry();
    let agent = r.list_by_category(CommandCategory::Agent);
    assert!(agent.is_empty());
}

// --- PingCommand ---

#[tokio::test]
async fn ping_command_returns_pong() {
    let r = make_registry();
    let cmd = r.get("ping").expect("ping must be registered");
    let result = cmd
        .execute(CommandArgs::new(), &CommandContext::new())
        .await
        .expect("ping must not fail");
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.data["message"], "pong");
}

// --- VersionCommand ---

#[tokio::test]
async fn version_command_returns_non_empty_version() {
    let r = make_registry();
    let cmd = r.get("version").expect("version must be registered");
    let result = cmd
        .execute(CommandArgs::new(), &CommandContext::new())
        .await
        .expect("version must not fail");
    assert_eq!(result.exit_code, 0);
    let v = result.data["version"].as_str().expect("version field must be a string");
    assert!(!v.is_empty(), "version string must not be empty");
}

// --- Unknown command → CommandError::NotFound ---

#[test]
fn unknown_command_not_found() {
    let r = make_registry();
    let result = r.get("nope");
    assert!(result.is_none());
    // Simulate the caller producing a NotFound error:
    let err = CommandError::NotFound("nope".into());
    assert!(err.to_string().contains("nope"));
}
