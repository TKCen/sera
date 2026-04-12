use sera_types::session::SessionState;

#[test]
fn session_state_new_variants_serde() {
    let variants = vec![
        (SessionState::Spawning, "\"spawning\""),
        (SessionState::TrustRequired, "\"trust_required\""),
        (SessionState::ReadyForPrompt, "\"ready_for_prompt\""),
        (SessionState::Paused, "\"paused\""),
        (SessionState::Shadow, "\"shadow\""),
    ];
    for (state, expected_json) in variants {
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, expected_json);
        let parsed: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, state);
    }
}

#[test]
fn shadow_session_valid_transitions() {
    // Created → Shadow OK
    assert!(SessionState::Created.can_transition_to(SessionState::Shadow));
    // Shadow → Destroyed OK
    assert!(SessionState::Shadow.can_transition_to(SessionState::Destroyed));
    // Shadow → Active REJECTED
    assert!(!SessionState::Shadow.can_transition_to(SessionState::Active));
}
