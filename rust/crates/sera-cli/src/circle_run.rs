use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::{circle_replay, sha256_hex};
use sera_types::circle_validator::validate_proof_bundle;

pub fn print_circle_run_footer(verdict: &str, bundle_sha256: &str) {
    println!("circle-run: {verdict} {bundle_sha256}");
}

pub fn run_circle_run(
    spec: Option<PathBuf>,
    capture_dir: Option<PathBuf>,
    bundle_out: Option<PathBuf>,
    json_out: bool,
) -> i32 {
    let Some(spec) = spec else {
        eprintln!("missing required --spec <PATH> for Circle run");
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    };
    let Some(capture_dir) = capture_dir else {
        eprintln!("missing required --capture-dir <DIR> for Circle run");
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    };
    let Some(bundle_out) = bundle_out else {
        eprintln!("missing required --bundle-out <PATH> for Circle run");
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    };

    if let Err(err) = materialize_run_fixtures(&spec, &capture_dir) {
        eprintln!("failed to capture Circle run fixtures: {err}");
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    }
    if let Err(err) = remove_colocated_replay_metadata(&capture_dir) {
        eprintln!("failed to prepare Circle run capture directory: {err}");
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    }

    let bundle = match circle_replay::build_replay_bundle(&capture_dir) {
        Ok(bundle) => bundle,
        Err(err) => {
            eprintln!("failed to replay captured Circle run: {err}");
            print_circle_run_footer("USAGE_ERROR", "unknown");
            return 3;
        }
    };

    let bytes = match serde_json::to_vec_pretty(&bundle) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("failed to serialize Circle run bundle: {err}");
            print_circle_run_footer("USAGE_ERROR", "unknown");
            return 3;
        }
    };
    let bundle_sha256 = sha256_hex(&bytes);

    if let Some(parent) = bundle_out
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "failed to create Circle run output directory {}: {err}",
            parent.display()
        );
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    }

    if let Err(err) = std::fs::write(&bundle_out, &bytes) {
        eprintln!(
            "failed to write Circle run bundle {}: {err}",
            bundle_out.display()
        );
        print_circle_run_footer("USAGE_ERROR", "unknown");
        return 3;
    }

    match validate_proof_bundle(&bundle) {
        Ok(()) => {
            if json_out {
                println!(
                    "{}",
                    json!({
                        "verdict": "PASS",
                        "bundle_sha256": bundle_sha256,
                        "spec": spec.display().to_string(),
                        "capture_dir": capture_dir.display().to_string(),
                        "bundle_out": bundle_out.display().to_string(),
                        "entries": bundle.entries.len(),
                        "execution_receipts": bundle.execution_receipts.len(),
                        "lineage_edges": bundle.lineage.len(),
                        "peer_challenges": bundle.peer_challenges.len(),
                        "circle_id": bundle.circle_id,
                        "run_id": bundle.run_id,
                    })
                );
            } else {
                println!(
                    "Circle run PASS: captured {} role fixtures, wrote {} entries={} receipts={} peer_challenges={} run_id={} circle_id={}",
                    bundle.roster.len(),
                    bundle_out.display(),
                    bundle.entries.len(),
                    bundle.execution_receipts.len(),
                    bundle.peer_challenges.len(),
                    bundle.run_id,
                    bundle.circle_id
                );
            }
            print_circle_run_footer("PASS", &bundle_sha256);
            0
        }
        Err(errors) => {
            if json_out {
                println!(
                    "{}",
                    json!({
                        "verdict": "FAIL",
                        "bundle_sha256": bundle_sha256,
                        "spec": spec.display().to_string(),
                        "capture_dir": capture_dir.display().to_string(),
                        "bundle_out": bundle_out.display().to_string(),
                        "entries": bundle.entries.len(),
                        "execution_receipts": bundle.execution_receipts.len(),
                        "lineage_edges": bundle.lineage.len(),
                        "peer_challenges": bundle.peer_challenges.len(),
                        "circle_id": bundle.circle_id,
                        "run_id": bundle.run_id,
                        "errors": errors.iter().map(|e| format!("{:?}", e.kind)).collect::<Vec<_>>(),
                    })
                );
            } else {
                eprintln!(
                    "Circle run FAIL: wrote {} with {} validation error(s)",
                    bundle_out.display(),
                    errors.len()
                );
                for error in &errors {
                    eprintln!("- {:?}", error.kind);
                }
            }
            print_circle_run_footer("FAIL", &bundle_sha256);
            1
        }
    }
}

fn materialize_run_fixtures(spec_path: &Path, capture_dir: &Path) -> Result<(), String> {
    let spec = read_spec(spec_path)?;
    let roles = spec
        .get("roles")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{} must contain roles[]", spec_path.display()))?;
    if roles.is_empty() {
        return Err(format!("{} roles[] is empty", spec_path.display()));
    }

    std::fs::create_dir_all(capture_dir).map_err(|err| {
        format!(
            "failed to create Circle run capture directory {}: {err}",
            capture_dir.display()
        )
    })?;

    let mut summary = spec.clone();
    let summary_obj = summary
        .as_object_mut()
        .ok_or_else(|| format!("{} must be a JSON/YAML object", spec_path.display()))?;
    let mut summary_roles = Vec::with_capacity(roles.len());
    let mut role_fixtures = Vec::with_capacity(roles.len());
    let mut seen_role_ids = HashSet::new();

    for role in roles {
        let role_id = required_str(role, "role_id")?;
        validate_role_id(role_id, &mut seen_role_ids)?;
        let timestamp = role
            .get("ended_at")
            .and_then(Value::as_str)
            .or_else(|| role.get("timestamp").and_then(Value::as_str))
            .ok_or_else(|| format!("role {role_id} must contain ended_at or timestamp"))?;
        let answer = role
            .get("answer_cleaned")
            .and_then(Value::as_str)
            .or_else(|| role.get("answer").and_then(Value::as_str))
            .ok_or_else(|| format!("role {role_id} must contain answer_cleaned or answer"))?;
        if answer.trim().is_empty() {
            return Err(format!("role {role_id} answer is blank"));
        }

        let mut role_fixture = role.clone();
        role_fixture["role_id"] = json!(role_id);
        role_fixture["ended_at"] = json!(timestamp);
        if role_fixture.get("answer_cleaned").is_none() {
            role_fixture["answer_cleaned"] = json!(answer);
        }
        if role_fixture.get("returncode").is_none() {
            role_fixture["returncode"] = json!(0);
        }
        if role_fixture.get("duration_ms").is_none() {
            role_fixture["duration_ms"] = json!(0);
        }
        if role_fixture.get("session_id").is_none() {
            role_fixture["session_id"] = json!(format!("circle-run-{role_id}"));
        }
        if role_fixture.get("action").is_none() {
            role_fixture["action"] = json!("sera_circle_run_role_capture");
        }

        let role_fixture_bytes = serde_json::to_vec_pretty(&role_fixture)
            .map_err(|err| format!("failed to serialize role fixture {role_id}: {err}"))?;
        summary_roles.push(role_summary(&role_fixture));
        role_fixtures.push((role_id.to_string(), role_fixture_bytes));
    }

    summary_obj.insert("roles".to_string(), Value::Array(summary_roles));
    summary_obj.remove("artifact");
    let summary_bytes = serde_json::to_vec_pretty(&summary)
        .map_err(|err| format!("failed to serialize summary.json: {err}"))?;

    for (role_id, role_fixture_bytes) in role_fixtures {
        std::fs::write(
            capture_dir.join(format!("{role_id}.json")),
            role_fixture_bytes,
        )
        .map_err(|err| format!("failed to write role fixture {role_id}: {err}"))?;
    }
    std::fs::write(capture_dir.join("summary.json"), summary_bytes)
        .map_err(|err| format!("failed to write summary.json: {err}"))?;

    Ok(())
}

fn role_summary(role: &Value) -> Value {
    let fields = [
        "role_id",
        "role",
        "provider",
        "provider_cli",
        "model",
        "session_id",
        "returncode",
        "duration_ms",
        "answer_had_harness_warnings",
    ];
    let mut out = serde_json::Map::new();
    for field in fields {
        if let Some(value) = role.get(field) {
            out.insert(field.to_string(), value.clone());
        }
    }
    Value::Object(out)
}

fn remove_colocated_replay_metadata(capture_dir: &Path) -> Result<(), String> {
    let stale_bundle = capture_dir.join("proof_bundle.json");
    match std::fs::remove_file(&stale_bundle) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!(
            "failed to remove {}: {err}",
            stale_bundle.display()
        )),
    }
}

fn validate_role_id(role_id: &str, seen: &mut HashSet<String>) -> Result<(), String> {
    if role_id.trim().is_empty() || role_id.trim() != role_id {
        return Err(format!(
            "unsafe role_id {role_id:?}: must be a nonblank filename atom"
        ));
    }
    if role_id.contains('/') || role_id.contains('\\') {
        return Err(format!(
            "unsafe role_id {role_id:?}: path separators are not allowed"
        ));
    }
    let mut components = Path::new(role_id).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => {}
        _ => {
            return Err(format!(
                "unsafe role_id {role_id:?}: must be a single filename atom"
            ));
        }
    }
    let normalized_role_id = role_id.to_lowercase();
    if matches!(normalized_role_id.as_str(), "summary" | "proof_bundle") {
        return Err(format!("reserved role_id {role_id:?}"));
    }
    if !seen.insert(normalized_role_id) {
        return Err(format!("duplicate role_id {role_id:?}"));
    }
    Ok(())
}

fn read_spec(path: &Path) -> Result<Value, String> {
    let bytes =
        std::fs::read(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("yaml") | Some("yml") => serde_yaml::from_slice(&bytes)
            .map_err(|err| format!("failed to parse {}: {err}", path.display())),
        _ => serde_json::from_slice(&bytes)
            .map_err(|err| format!("failed to parse {}: {err}", path.display())),
    }
}

fn required_str<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("role missing string field {field}"))
}
