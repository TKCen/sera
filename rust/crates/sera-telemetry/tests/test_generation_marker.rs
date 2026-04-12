//! Verify GenerationMarker JSON roundtrip and label non-empty invariant.

use sera_telemetry::generation::{BuildIdentity, GenerationLabel, GenerationMarker};
use time::OffsetDateTime;

fn make_marker(label: &str) -> GenerationMarker {
    GenerationMarker {
        label: GenerationLabel(label.to_string()),
        binary_identity: BuildIdentity {
            version: "0.1.0".to_string(),
            commit: "abc1234".to_string(),
            build_time: OffsetDateTime::UNIX_EPOCH,
            signer_fingerprint: [0u8; 32],
            constitution_hash: [42u8; 32],
        },
        started_at: OffsetDateTime::UNIX_EPOCH,
    }
}

#[test]
fn generation_marker_json_roundtrip() {
    let marker = make_marker("mvs-0.1.0");
    let json = serde_json::to_string(&marker).expect("serialize");
    let restored: GenerationMarker = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(restored.label.0, "mvs-0.1.0");
    assert_eq!(restored.binary_identity.version, "0.1.0");
    assert_eq!(restored.binary_identity.commit, "abc1234");
    assert_eq!(restored.binary_identity.constitution_hash, [42u8; 32]);
}

#[test]
fn label_non_empty() {
    let marker = make_marker("sera-2.0");
    assert!(!marker.label.0.is_empty());
}
