use std::path::{Path, PathBuf};

use sera_types::{circle::CollaborationProofBundle, circle_validator::validate_proof_bundle};
use serde_json::{Value, json};

use crate::sha256_hex;

pub fn print_circle_replay_footer(verdict: &str, bundle_sha256: &str) {
    println!("circle-replay: {verdict} {bundle_sha256}");
}

pub fn run_circle_replay(
    fixture_dir: Option<PathBuf>,
    bundle_out: Option<PathBuf>,
    json_out: bool,
) -> i32 {
    let Some(fixture_dir) = fixture_dir else {
        eprintln!("missing required --fixture-dir <DIR> for Circle replay");
        print_circle_replay_footer("USAGE_ERROR", "unknown");
        return 3;
    };

    let Some(bundle_out) = bundle_out else {
        eprintln!("missing required --bundle-out <PATH> for Circle replay");
        print_circle_replay_footer("USAGE_ERROR", "unknown");
        return 3;
    };

    let bundle = match build_replay_bundle(&fixture_dir) {
        Ok(bundle) => bundle,
        Err(err) => {
            eprintln!("failed to build Circle replay bundle: {err}");
            print_circle_replay_footer("USAGE_ERROR", "unknown");
            return 3;
        }
    };

    let bytes = match serde_json::to_vec_pretty(&bundle) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("failed to serialize Circle replay bundle: {err}");
            print_circle_replay_footer("USAGE_ERROR", "unknown");
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
            "failed to create Circle replay output directory {}: {err}",
            parent.display()
        );
        print_circle_replay_footer("USAGE_ERROR", "unknown");
        return 3;
    }

    if let Err(err) = std::fs::write(&bundle_out, &bytes) {
        eprintln!(
            "failed to write Circle replay bundle {}: {err}",
            bundle_out.display()
        );
        print_circle_replay_footer("USAGE_ERROR", "unknown");
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
                        "bundle_out": bundle_out.display().to_string(),
                        "entries": bundle.entries.len(),
                        "execution_receipts": bundle.execution_receipts.len(),
                        "lineage_edges": bundle.lineage.len(),
                        "circle_id": bundle.circle_id,
                        "run_id": bundle.run_id,
                    })
                );
            } else {
                println!(
                    "Circle replay bundle PASS: wrote {} entries={} receipts={} lineage={} run_id={} circle_id={}",
                    bundle_out.display(),
                    bundle.entries.len(),
                    bundle.execution_receipts.len(),
                    bundle.lineage.len(),
                    bundle.run_id,
                    bundle.circle_id
                );
            }
            print_circle_replay_footer("PASS", &bundle_sha256);
            0
        }
        Err(errors) => {
            if json_out {
                println!(
                    "{}",
                    json!({
                        "verdict": "FAIL",
                        "bundle_sha256": bundle_sha256,
                        "bundle_out": bundle_out.display().to_string(),
                        "entries": bundle.entries.len(),
                        "execution_receipts": bundle.execution_receipts.len(),
                        "lineage_edges": bundle.lineage.len(),
                        "circle_id": bundle.circle_id,
                        "run_id": bundle.run_id,
                        "errors": errors.iter().map(|e| format!("{:?}", e.kind)).collect::<Vec<_>>(),
                    })
                );
            } else {
                eprintln!(
                    "Circle replay bundle FAIL: wrote {} with {} validation error(s)",
                    bundle_out.display(),
                    errors.len()
                );
                for error in &errors {
                    eprintln!("- {:?}", error.kind);
                }
            }
            print_circle_replay_footer("FAIL", &bundle_sha256);
            1
        }
    }
}

fn build_replay_bundle(fixture_dir: &Path) -> Result<CollaborationProofBundle, String> {
    let summary_path = fixture_dir.join("summary.json");
    let summary = read_json(&summary_path)?;
    let metadata_source = replay_metadata_source(fixture_dir, &summary)?;
    let roles = summary
        .get("roles")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{} must contain roles[]", summary_path.display()))?;

    if roles.is_empty() {
        return Err(format!("{} roles[] is empty", summary_path.display()));
    }

    let mut roster = Vec::with_capacity(roles.len());
    let mut entries = Vec::with_capacity(roles.len());
    let mut receipts = Vec::with_capacity(roles.len());
    let mut last_referee: Option<(String, String, String)> = None;

    for (index, role_summary) in roles.iter().enumerate() {
        let role_id = required_str(role_summary, "role_id")?.to_string();
        let role_fixture = read_json(&fixture_dir.join(format!("{role_id}.json")))?;
        let role_label = role_fixture
            .get("role")
            .and_then(Value::as_str)
            .or_else(|| role_summary.get("role").and_then(Value::as_str))
            .unwrap_or_else(|| default_role_label(&role_id));
        let provider_cli = role_fixture
            .get("provider_cli")
            .and_then(Value::as_str)
            .or_else(|| role_summary.get("provider_cli").and_then(Value::as_str))
            .unwrap_or("unknown");
        let model = role_fixture
            .get("model")
            .and_then(Value::as_str)
            .or_else(|| role_summary.get("model").and_then(Value::as_str))
            .unwrap_or("unknown");
        let provider = role_fixture
            .get("provider")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| canonical_provider(provider_cli, model));
        let session_id = role_fixture
            .get("session_id")
            .and_then(Value::as_str)
            .or_else(|| role_summary.get("session_id").and_then(Value::as_str))
            .unwrap_or("unknown");
        let returncode = role_fixture
            .get("returncode")
            .and_then(Value::as_i64)
            .or_else(|| role_summary.get("returncode").and_then(Value::as_i64))
            .unwrap_or(0);
        let duration_ms = role_fixture
            .get("duration_ms")
            .and_then(Value::as_u64)
            .or_else(|| role_summary.get("duration_ms").and_then(Value::as_u64))
            .unwrap_or(0);
        let timestamp = role_fixture
            .get("ended_at")
            .and_then(Value::as_str)
            .or_else(|| role_fixture.get("timestamp").and_then(Value::as_str))
            .ok_or_else(|| format!("{role_id}.json must contain ended_at or timestamp"))?;
        let response = role_fixture
            .get("answer_cleaned")
            .and_then(Value::as_str)
            .or_else(|| role_fixture.get("answer").and_then(Value::as_str))
            .ok_or_else(|| format!("{role_id}.json must contain answer_cleaned or answer"))?;

        roster.push(json!({
            "participant_id": role_id,
            "role": role_label,
        }));

        entries.push(json!({
            "entry_id": (index + 1) as u64,
            "author": role_id,
            "timestamp": timestamp,
            "artifact_type": artifact_type_for(&role_id, index, roles.len()),
            "payload": {
                "role": role_id,
                "provider": provider,
                "provider_cli": provider_cli,
                "model": model,
                "session_id": session_id,
                "response": response,
                "harness_warning_cleaned_from_raw": role_fixture
                    .get("answer_had_harness_warnings")
                    .and_then(Value::as_bool)
                    .or_else(|| role_summary.get("harness_warning_cleaned_from_raw").and_then(Value::as_bool))
                    .unwrap_or(false),
            }
        }));

        let outcome = if returncode == 0 {
            json!("success")
        } else {
            json!({ "failure": { "reason": format!("fixture returncode {returncode}") } })
        };
        receipts.push(json!({
            "receipt_id": format!("rcpt_{:03}_{role_id}", index + 1),
            "executor": role_id,
            "timestamp": timestamp,
            "action": role_fixture.get("action").and_then(Value::as_str).unwrap_or("hermes_chat"),
            "parameters": {
                "provider": provider,
                "provider_cli": provider_cli,
                "model": model,
                "session_id": session_id,
                "duration_ms": duration_ms,
                "returncode": returncode,
                "harness_warning_cleaned_from_raw": role_fixture
                    .get("answer_had_harness_warnings")
                    .and_then(Value::as_bool)
                    .or_else(|| role_summary.get("harness_warning_cleaned_from_raw").and_then(Value::as_bool))
                    .unwrap_or(false),
            },
            "outcome": outcome,
        }));

        if role_id.contains("referee") || index + 1 == roles.len() {
            last_referee = Some((role_id, timestamp.to_string(), response.to_string()));
        }
    }

    let lineage: Vec<Value> = (1..roles.len())
        .map(|index| {
            json!({
                "from_entry_id": index as u64,
                "to_entry_id": (index + 1) as u64,
                "relation": lineage_relation_for(index, roles.len()),
            })
        })
        .collect();

    let verdict_type = summary.get("verdict_type").cloned().or_else(|| {
        metadata_source
            .as_ref()
            .and_then(|metadata| metadata.get("verdict"))
            .and_then(|verdict| verdict.get("verdict_type"))
            .cloned()
    });

    let verdict = verdict_type.map(|verdict_type| {
        let (reviewer, timestamp, rationale) = last_referee.clone().unwrap_or_else(|| {
            (
                "unknown_referee".to_string(),
                "1970-01-01T00:00:00Z".to_string(),
                "No referee rationale captured in replay fixture.".to_string(),
            )
        });
        json!({
            "reviewer": reviewer,
            "timestamp": timestamp,
            "verdict_type": verdict_type,
            "rationale": rationale,
        })
    });

    let mut bundle = json!({
        "run_id": str_from_summary_or_metadata(&summary, metadata_source.as_ref(), "run_id")
            .unwrap_or("circle-replay-fixture-run"),
        "circle_id": str_from_summary_or_metadata(&summary, metadata_source.as_ref(), "circle_id")
            .unwrap_or("circle-replay-fixture"),
        "objective": str_from_summary_or_metadata(&summary, metadata_source.as_ref(), "objective")
            .unwrap_or("Replay Circle fixture transcript into a collaboration proof bundle."),
        "success_metric": value_from_summary_or_metadata(&summary, metadata_source.as_ref(), "success_metric").unwrap_or_else(|| json!({
            "kind": "inline",
            "description": "Replay fixture produces a structurally valid CollaborationProofBundle."
        })),
        "roster": roster,
        "entries": entries,
        "lineage": lineage,
        "execution_receipts": receipts,
    });

    if let Some(snapshot) =
        value_from_summary_or_metadata(&summary, metadata_source.as_ref(), "budget_snapshot")
    {
        bundle["budget_snapshot"] = snapshot.clone();
    }
    if let Some(verdict) = verdict {
        bundle["verdict"] = verdict;
    }

    serde_json::from_value(bundle)
        .map_err(|err| format!("replay produced invalid bundle shape: {err}"))
}

fn replay_metadata_source(fixture_dir: &Path, summary: &Value) -> Result<Option<Value>, String> {
    let explicit = summary
        .get("artifact")
        .and_then(Value::as_str)
        .map(|artifact| {
            let path = PathBuf::from(artifact);
            if path.is_absolute() {
                path
            } else {
                fixture_dir.join(path)
            }
        });
    let local = fixture_dir.join("proof_bundle.json");
    let candidate = explicit.or_else(|| local.exists().then_some(local));

    match candidate {
        Some(path) if path.exists() => read_json(&path).map(Some),
        Some(path) => Err(format!(
            "summary artifact metadata source does not exist: {}",
            path.display()
        )),
        None => Ok(None),
    }
}

fn str_from_summary_or_metadata<'a>(
    summary: &'a Value,
    metadata: Option<&'a Value>,
    field: &str,
) -> Option<&'a str> {
    summary.get(field).and_then(Value::as_str).or_else(|| {
        metadata
            .and_then(|metadata| metadata.get(field))
            .and_then(Value::as_str)
    })
}

fn value_from_summary_or_metadata(
    summary: &Value,
    metadata: Option<&Value>,
    field: &str,
) -> Option<Value> {
    summary
        .get(field)
        .cloned()
        .or_else(|| metadata.and_then(|metadata| metadata.get(field)).cloned())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes =
        std::fs::read(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn required_str<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("role summary missing string field {field}"))
}

fn canonical_provider(provider_cli: &str, model: &str) -> String {
    let provider = provider_cli.trim().to_ascii_lowercase();
    let model = model.trim().to_ascii_lowercase();

    if provider.contains("ollama") {
        return "ollama".to_string();
    }
    let provider_compact = provider.replace(['-', '_'], "");
    if provider.contains("local")
        || provider_compact.contains("lmstudio")
        || provider == "custom"
        || model.contains("gemma")
        || model.contains("qwen")
        || model.contains("llama")
        || model.contains("mistral")
    {
        return "local-lmstudio".to_string();
    }

    provider
}

fn default_role_label(role_id: &str) -> &str {
    if role_id.contains("specifier") {
        "specifier"
    } else if role_id.contains("builder") {
        "builder"
    } else if role_id.contains("critic") {
        "critic"
    } else if role_id.contains("referee") {
        "referee/integrator"
    } else {
        "perspective"
    }
}

fn artifact_type_for(role_id: &str, index: usize, total: usize) -> &'static str {
    if role_id.contains("specifier") || index == 0 {
        "specification"
    } else if role_id.contains("builder") {
        "implementation_plan"
    } else if role_id.contains("critic") {
        "critique"
    } else if role_id.contains("referee") || index + 1 == total {
        "verdict"
    } else {
        "perspective"
    }
}

fn lineage_relation_for(index: usize, total: usize) -> &'static str {
    if index + 1 == total {
        "resolves"
    } else if index >= 2 {
        "criticizes"
    } else {
        "derives_from"
    }
}
