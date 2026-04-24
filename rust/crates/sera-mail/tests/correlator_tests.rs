//! Integration tests for the mail correlator.
//!
//! Covers the B1 (headers) / B2 (body nonce) / B3 (drop) ladder plus the
//! envelope-index lifecycle (TTL, isolation, forget).

use std::sync::Arc;
use std::time::Duration;

use sera_mail::{
    generate_nonce, parse_raw_message, CorrelationOutcome, CorrelationTier, DropReason,
    EnvelopeIndex, HeaderMailCorrelator, InMemoryEnvelopeIndex, InMemoryMailLookup, IssuanceHook,
    MailCorrelator, OutboundEnvelope,
};
use sera_workflow::task::{MailEvent, MailThreadId};
use sera_workflow::MailLookup;

// ── Helpers ─────────────────────────────────────────────────────────────────

fn make_envelope(gate: &str, thread: &str, nonce: &str) -> OutboundEnvelope {
    OutboundEnvelope::new(
        sera_mail::envelope::GateId::new(gate),
        MailThreadId::new(thread),
        nonce.to_string(),
        vec!["alice@example.com".to_string()],
        "Approval needed".to_string(),
        "Please approve.".to_string(),
    )
}

fn reply_with_irt(irt: &str, body: &str) -> Vec<u8> {
    format!(
        "From: alice@example.com\r\n\
         To: sera@example.com\r\n\
         Subject: Re: Approval needed\r\n\
         Message-ID: <reply-{uuid}@example.com>\r\n\
         In-Reply-To: <{irt}>\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         {body}\r\n",
        uuid = uuid::Uuid::new_v4(),
        irt = irt,
        body = body
    )
    .into_bytes()
}

fn reply_with_refs(refs: &[&str], body: &str) -> Vec<u8> {
    let refs_header = refs
        .iter()
        .map(|r| format!("<{r}>"))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "From: alice@example.com\r\n\
         To: sera@example.com\r\n\
         Subject: Re: Approval needed\r\n\
         Message-ID: <reply-{uuid}@example.com>\r\n\
         References: {refs_header}\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         {body}\r\n",
        uuid = uuid::Uuid::new_v4(),
        refs_header = refs_header,
        body = body
    )
    .into_bytes()
}

fn reply_no_headers(body: &str) -> Vec<u8> {
    format!(
        "From: alice@example.com\r\n\
         To: sera@example.com\r\n\
         Subject: Re: Approval needed\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         {body}\r\n",
        body = body
    )
    .into_bytes()
}

// ── B1: header correlation ──────────────────────────────────────────────────

#[tokio::test]
async fn b1_in_reply_to_header_resolves() {
    let correlator = HeaderMailCorrelator::new_in_memory();
    let env = make_envelope("gate-1", "sera-gate-1@sera.local", "NONCE1234567890A");
    correlator.on_issued(&env).await.unwrap();

    let raw = reply_with_irt("sera-gate-1@sera.local", "Approved.");
    let msg = parse_raw_message(&raw).unwrap();

    let outcome = correlator.correlate(&msg).await.unwrap();
    match outcome {
        CorrelationOutcome::Resolved { gate_id, thread_id, tier } => {
            assert_eq!(gate_id.as_str(), "gate-1");
            assert_eq!(thread_id.as_str(), "sera-gate-1@sera.local");
            assert_eq!(tier, CorrelationTier::B1Headers);
        }
        other => panic!("expected Resolved, got {other:?}"),
    }
}

#[tokio::test]
async fn b1_references_chain_resolves() {
    // Some clients put the gate's Message-ID only in References (e.g.
    // deep-threaded replies). Correlator must walk the chain.
    let correlator = HeaderMailCorrelator::new_in_memory();
    let env = make_envelope("gate-chain", "sera-gate-chain@sera.local", "CHAINNONCE123456");
    correlator.on_issued(&env).await.unwrap();

    let raw = reply_with_refs(
        &["older@example.com", "sera-gate-chain@sera.local", "newest@example.com"],
        "Approved.",
    );
    let msg = parse_raw_message(&raw).unwrap();

    let outcome = correlator.correlate(&msg).await.unwrap();
    assert!(matches!(
        outcome,
        CorrelationOutcome::Resolved { tier: CorrelationTier::B1Headers, .. }
    ));
}

#[tokio::test]
async fn b1_wins_over_b2_when_both_present() {
    // Even if the body has a nonce for a *different* gate, a valid header
    // match takes precedence.
    let correlator = HeaderMailCorrelator::new_in_memory();
    let env_a = make_envelope("gate-a", "sera-gate-a@sera.local", "AAAAAAAA11111111");
    let env_b = make_envelope("gate-b", "sera-gate-b@sera.local", "BBBBBBBB22222222");
    correlator.on_issued(&env_a).await.unwrap();
    correlator.on_issued(&env_b).await.unwrap();

    // Reply has In-Reply-To for gate-a but body footer nonce for gate-b.
    let raw = reply_with_irt("sera-gate-a@sera.local", "Approved.\n[SERA:nonce=BBBBBBBB22222222]");
    let msg = parse_raw_message(&raw).unwrap();

    let outcome = correlator.correlate(&msg).await.unwrap();
    match outcome {
        CorrelationOutcome::Resolved { gate_id, tier, .. } => {
            assert_eq!(gate_id.as_str(), "gate-a");
            assert_eq!(tier, CorrelationTier::B1Headers);
        }
        other => panic!("expected Resolved, got {other:?}"),
    }
}

// ── B2: body-nonce correlation ─────────────────────────────────────────────

#[tokio::test]
async fn b2_body_nonce_resolves_without_headers() {
    let correlator = HeaderMailCorrelator::new_in_memory();
    let env = make_envelope("gate-b2", "sera-gate-b2@sera.local", "B2NONCE1234567890");
    correlator.on_issued(&env).await.unwrap();

    let raw = reply_no_headers("Approved.\n[SERA:nonce=B2NONCE1234567890]");
    let msg = parse_raw_message(&raw).unwrap();
    assert!(msg.in_reply_to.is_none());

    let outcome = correlator.correlate(&msg).await.unwrap();
    match outcome {
        CorrelationOutcome::Resolved { gate_id, thread_id, tier } => {
            assert_eq!(gate_id.as_str(), "gate-b2");
            assert_eq!(thread_id.as_str(), "sera-gate-b2@sera.local");
            assert_eq!(tier, CorrelationTier::B2BodyNonce);
        }
        other => panic!("expected Resolved, got {other:?}"),
    }
}

#[tokio::test]
async fn b2_unknown_nonce_drops() {
    // Nonce in body but no matching envelope → fall through to B3.
    let correlator = HeaderMailCorrelator::new_in_memory();

    let raw = reply_no_headers("Approved.\n[SERA:nonce=UNREGISTERED12345]");
    let msg = parse_raw_message(&raw).unwrap();

    let outcome = correlator.correlate(&msg).await.unwrap();
    assert_eq!(
        outcome,
        CorrelationOutcome::Dropped { reason: DropReason::NoMatch }
    );
}

#[tokio::test]
async fn b2_handles_duplicate_footer_in_quoted_reply() {
    // Gmail often quotes the original body, so the nonce can appear twice.
    let correlator = HeaderMailCorrelator::new_in_memory();
    let env = make_envelope("gate-dup", "sera-gate-dup@sera.local", "DUPNONCE12345678");
    correlator.on_issued(&env).await.unwrap();

    let body = "Approved.\n\n> On Mon, SERA wrote:\n> [SERA:nonce=DUPNONCE12345678]\n\n[SERA:nonce=DUPNONCE12345678]";
    let raw = reply_no_headers(body);
    let msg = parse_raw_message(&raw).unwrap();

    let outcome = correlator.correlate(&msg).await.unwrap();
    assert!(matches!(
        outcome,
        CorrelationOutcome::Resolved { tier: CorrelationTier::B2BodyNonce, .. }
    ));
}

// ── B3: drop ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn b3_no_match_unknown_thread_drops() {
    let correlator = HeaderMailCorrelator::new_in_memory();

    let raw = reply_with_irt("stranger@example.com", "Hi there.");
    let msg = parse_raw_message(&raw).unwrap();

    let outcome = correlator.correlate(&msg).await.unwrap();
    assert_eq!(
        outcome,
        CorrelationOutcome::Dropped { reason: DropReason::NoMatch }
    );
}

#[tokio::test]
async fn b3_no_headers_no_nonce_drops() {
    let correlator = HeaderMailCorrelator::new_in_memory();

    let raw = reply_no_headers("Just saying hi.");
    let msg = parse_raw_message(&raw).unwrap();

    let outcome = correlator.correlate(&msg).await.unwrap();
    assert_eq!(
        outcome,
        CorrelationOutcome::Dropped { reason: DropReason::NoMatch }
    );
}

// ── Envelope-index lifecycle ────────────────────────────────────────────────

#[tokio::test]
async fn index_isolates_two_envelopes() {
    let correlator = HeaderMailCorrelator::new_in_memory();
    let env_a = make_envelope("gate-a", "sera-a@sera.local", "AAAAAAAA11111111");
    let env_b = make_envelope("gate-b", "sera-b@sera.local", "BBBBBBBB22222222");
    correlator.on_issued(&env_a).await.unwrap();
    correlator.on_issued(&env_b).await.unwrap();

    let raw_a = reply_with_irt("sera-a@sera.local", "Approved A.");
    let msg_a = parse_raw_message(&raw_a).unwrap();
    let outcome_a = correlator.correlate(&msg_a).await.unwrap();
    assert!(matches!(
        outcome_a,
        CorrelationOutcome::Resolved { ref gate_id, .. } if gate_id.as_str() == "gate-a"
    ));

    let raw_b = reply_with_irt("sera-b@sera.local", "Approved B.");
    let msg_b = parse_raw_message(&raw_b).unwrap();
    let outcome_b = correlator.correlate(&msg_b).await.unwrap();
    assert!(matches!(
        outcome_b,
        CorrelationOutcome::Resolved { ref gate_id, .. } if gate_id.as_str() == "gate-b"
    ));
}

#[tokio::test]
async fn index_expired_envelope_drops() {
    // TTL is 0ns → any subsequent lookup sees the envelope as expired.
    let index = Arc::new(InMemoryEnvelopeIndex::new(Duration::from_nanos(1)));
    let correlator = HeaderMailCorrelator::new(index.clone(), None);

    let env = make_envelope("gate-expire", "sera-expire@sera.local", "EXPIRENONCE12345");
    correlator.on_issued(&env).await.unwrap();

    // Give the clock a moment to move past the TTL.
    std::thread::sleep(Duration::from_millis(5));

    let raw = reply_with_irt("sera-expire@sera.local", "Too late.");
    let msg = parse_raw_message(&raw).unwrap();
    let outcome = correlator.correlate(&msg).await.unwrap();
    assert_eq!(
        outcome,
        CorrelationOutcome::Dropped { reason: DropReason::NoMatch }
    );
}

#[tokio::test]
async fn index_forget_removes_envelope() {
    let index = Arc::new(InMemoryEnvelopeIndex::default());
    let correlator = HeaderMailCorrelator::new(index.clone(), None);

    let env = make_envelope("gate-forget", "sera-forget@sera.local", "FORGETNONCE12345");
    correlator.on_issued(&env).await.unwrap();
    assert_eq!(index.len(), 1);

    index.forget(&MailThreadId::new("sera-forget@sera.local")).unwrap();
    assert_eq!(index.len(), 0);

    let raw = reply_with_irt("sera-forget@sera.local", "Approved.");
    let msg = parse_raw_message(&raw).unwrap();
    let outcome = correlator.correlate(&msg).await.unwrap();
    assert!(matches!(
        outcome,
        CorrelationOutcome::Dropped { reason: DropReason::NoMatch }
    ));
}

#[tokio::test]
async fn index_register_is_idempotent_on_thread_id() {
    // Registering the same thread twice overwrites cleanly.
    let correlator = HeaderMailCorrelator::new_in_memory();
    let env_v1 = make_envelope("gate-v", "sera-v@sera.local", "V1NONCE123456789");
    let env_v2 = make_envelope("gate-v", "sera-v@sera.local", "V2NONCE123456789");
    correlator.on_issued(&env_v1).await.unwrap();
    correlator.on_issued(&env_v2).await.unwrap();

    // V1 nonce is no longer indexed — v2 replaced it. But header lookup still
    // resolves to gate-v.
    let raw_hdr = reply_with_irt("sera-v@sera.local", "Approved.");
    let msg_hdr = parse_raw_message(&raw_hdr).unwrap();
    let outcome_hdr = correlator.correlate(&msg_hdr).await.unwrap();
    assert!(matches!(
        outcome_hdr,
        CorrelationOutcome::Resolved { ref gate_id, .. } if gate_id.as_str() == "gate-v"
    ));

    // Old nonce is dropped.
    let raw_old = reply_no_headers("Approved.\n[SERA:nonce=V1NONCE123456789]");
    let msg_old = parse_raw_message(&raw_old).unwrap();
    let outcome_old = correlator.correlate(&msg_old).await.unwrap();
    assert_eq!(
        outcome_old,
        CorrelationOutcome::Dropped { reason: DropReason::NoMatch }
    );
}

// ── Nonce generation ────────────────────────────────────────────────────────

#[test]
fn generate_nonce_is_random_and_url_safe() {
    let a = generate_nonce();
    let b = generate_nonce();
    assert_ne!(a, b, "two fresh nonces should never collide");
    for n in [&a, &b] {
        assert!(!n.is_empty());
        assert!(n.len() >= 20, "128 bits b64url-no-pad is 22 chars");
        assert!(
            n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "nonce must be url-safe: {n}"
        );
    }
}

#[test]
fn outbound_envelope_appends_footer_once() {
    let env = OutboundEnvelope::new(
        sera_mail::envelope::GateId::new("g"),
        MailThreadId::new("t@sera.local"),
        "ABC123".to_string(),
        vec!["a@b.com".to_string()],
        "s".to_string(),
        "body".to_string(),
    );
    assert!(env.body.contains("[SERA:nonce=ABC123]"));

    // If the caller already included a footer we do not double-append.
    let env2 = OutboundEnvelope::new(
        sera_mail::envelope::GateId::new("g"),
        MailThreadId::new("t@sera.local"),
        "ABC123".to_string(),
        vec!["a@b.com".to_string()],
        "s".to_string(),
        "body\n[SERA:nonce=ABC123]".to_string(),
    );
    assert_eq!(env2.body.matches("[SERA:nonce=").count(), 1);
}

// ── Notify sink + MailLookup bridge ─────────────────────────────────────────

#[tokio::test]
async fn notify_sink_resolves_on_b1_and_lookup_reads_terminal_event() {
    let lookup = Arc::new(InMemoryMailLookup::new());
    let index = Arc::new(InMemoryEnvelopeIndex::default());
    let correlator = HeaderMailCorrelator::new(index.clone(), Some(lookup.clone()));

    let env = make_envelope("gate-n1", "sera-n1@sera.local", "N1NONCE123456789");
    correlator.on_issued(&env).await.unwrap();

    // Before the reply: lookup returns None (no events yet).
    assert!(lookup.thread_event(&MailThreadId::new("sera-n1@sera.local")).is_none());

    // Inbound reply → correlator pushes into lookup.
    let raw = reply_with_irt("sera-n1@sera.local", "Approved.");
    let msg = parse_raw_message(&raw).unwrap();
    let outcome = correlator.correlate(&msg).await.unwrap();
    assert!(matches!(outcome, CorrelationOutcome::Resolved { .. }));

    // Lookup now sees ReplyReceived.
    let ev = lookup.thread_event(&MailThreadId::new("sera-n1@sera.local"));
    assert_eq!(ev, Some(MailEvent::ReplyReceived));
}

#[tokio::test]
async fn notify_sink_resolves_on_b2_body_nonce() {
    let lookup = Arc::new(InMemoryMailLookup::new());
    let index = Arc::new(InMemoryEnvelopeIndex::default());
    let correlator = HeaderMailCorrelator::new(index, Some(lookup.clone()));

    let env = make_envelope("gate-n2", "sera-n2@sera.local", "N2NONCE123456789");
    correlator.on_issued(&env).await.unwrap();

    let raw = reply_no_headers("Approved.\n[SERA:nonce=N2NONCE123456789]");
    let msg = parse_raw_message(&raw).unwrap();
    let _ = correlator.correlate(&msg).await.unwrap();

    let ev = lookup.thread_event(&MailThreadId::new("sera-n2@sera.local"));
    assert_eq!(ev, Some(MailEvent::ReplyReceived));
}

#[tokio::test]
async fn notify_sink_not_invoked_on_drop() {
    let lookup = Arc::new(InMemoryMailLookup::new());
    let index = Arc::new(InMemoryEnvelopeIndex::default());
    let correlator = HeaderMailCorrelator::new(index, Some(lookup.clone()));

    let raw = reply_with_irt("stranger@example.com", "hi");
    let msg = parse_raw_message(&raw).unwrap();
    let outcome = correlator.correlate(&msg).await.unwrap();
    assert!(matches!(outcome, CorrelationOutcome::Dropped { .. }));

    assert_eq!(lookup.thread_count(), 0);
}

// ── InMemoryMailLookup tests (standalone) ───────────────────────────────────

#[tokio::test]
async fn lookup_events_after_filters_by_seq() {
    let lookup = InMemoryMailLookup::new();
    let tid = MailThreadId::new("thread-1@sera.local");

    lookup
        .notify(
            sera_mail::envelope::GateId::new("g"),
            &tid,
            MailEvent::Pending,
        )
        .unwrap();
    lookup
        .notify(
            sera_mail::envelope::GateId::new("g"),
            &tid,
            MailEvent::ReplyReceived,
        )
        .unwrap();
    lookup
        .notify(
            sera_mail::envelope::GateId::new("g"),
            &tid,
            MailEvent::Closed,
        )
        .unwrap();

    let all = lookup.events_after(&tid, 0);
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].event, MailEvent::Pending);
    assert_eq!(all[1].event, MailEvent::ReplyReceived);
    assert_eq!(all[2].event, MailEvent::Closed);
    assert!(all[0].seq < all[1].seq && all[1].seq < all[2].seq);

    // after = seq of event 1 → events 2 and 3 remain.
    let tail = lookup.events_after(&tid, all[0].seq);
    assert_eq!(tail.len(), 2);
    assert_eq!(tail[0].event, MailEvent::ReplyReceived);
}

#[tokio::test]
async fn lookup_thread_event_returns_latest() {
    // sera-workflow's scheduler only reads the current event; we must return
    // whichever one was recorded most recently.
    let lookup = InMemoryMailLookup::new();
    let tid = MailThreadId::new("thread-latest");

    lookup
        .notify(
            sera_mail::envelope::GateId::new("g"),
            &tid,
            MailEvent::Pending,
        )
        .unwrap();
    assert_eq!(lookup.thread_event(&tid), Some(MailEvent::Pending));

    lookup
        .notify(
            sera_mail::envelope::GateId::new("g"),
            &tid,
            MailEvent::ReplyReceived,
        )
        .unwrap();
    assert_eq!(lookup.thread_event(&tid), Some(MailEvent::ReplyReceived));

    lookup
        .notify(
            sera_mail::envelope::GateId::new("g"),
            &tid,
            MailEvent::Closed,
        )
        .unwrap();
    assert_eq!(lookup.thread_event(&tid), Some(MailEvent::Closed));
}

#[tokio::test]
async fn lookup_thread_event_unknown_is_none() {
    let lookup = InMemoryMailLookup::new();
    assert!(lookup.thread_event(&MailThreadId::new("never-seen")).is_none());
}

#[tokio::test]
async fn lookup_closed_is_terminal_for_scheduler() {
    // Sanity: a Closed event from the correlator side becomes terminal on the
    // scheduler side via is_mail_ready.
    let lookup = InMemoryMailLookup::new();
    let tid = MailThreadId::new("closed-thread");
    lookup
        .notify(
            sera_mail::envelope::GateId::new("g"),
            &tid,
            MailEvent::Closed,
        )
        .unwrap();

    let await_type = sera_workflow::task::AwaitType::Mail { thread_id: tid.clone() };
    assert!(sera_workflow::is_mail_ready(&await_type, &lookup));
}

// ── End-to-end ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn end_to_end_issue_correlate_ready() {
    // 1. Issue outbound envelope → records in index
    // 2. Simulate inbound reply → correlator resolves
    // 3. Lookup yields MailEvent::ReplyReceived
    // 4. is_mail_ready returns true
    let lookup = Arc::new(InMemoryMailLookup::new());
    let index = Arc::new(InMemoryEnvelopeIndex::default());
    let correlator = HeaderMailCorrelator::new(index, Some(lookup.clone()));

    let thread = "e2e-thread@sera.local";
    let env = make_envelope("e2e-gate", thread, "E2ENONCE12345678");
    correlator.on_issued(&env).await.unwrap();

    let raw = reply_with_irt(thread, "Approved.");
    let msg = parse_raw_message(&raw).unwrap();
    let outcome = correlator.correlate(&msg).await.unwrap();

    assert!(matches!(
        outcome,
        CorrelationOutcome::Resolved { tier: CorrelationTier::B1Headers, .. }
    ));

    let await_type =
        sera_workflow::task::AwaitType::Mail { thread_id: MailThreadId::new(thread) };
    assert!(sera_workflow::is_mail_ready(&await_type, &*lookup));
}

#[tokio::test]
async fn spoofed_sender_with_valid_message_id_still_resolves_by_headers() {
    // Threat-model note: transport-level spoofing (wrong From) does not
    // bypass the correlator — we route by Message-ID. Authenticity checks
    // live at the gate-handler layer.
    let correlator = HeaderMailCorrelator::new_in_memory();
    let env = make_envelope("gate-spoof", "sera-spoof@sera.local", "SPOOFNONCE123456");
    correlator.on_issued(&env).await.unwrap();

    // Reply claims to be from a different sender but references the correct
    // Message-ID.
    let raw = "From: attacker@evil.com\r\n\
         To: sera@example.com\r\n\
         Subject: Re: Approval needed\r\n\
         Message-ID: <r-123@attacker>\r\n\
         In-Reply-To: <sera-spoof@sera.local>\r\n\
         Content-Type: text/plain\r\n\
         \r\n\
         \"Approved\"\r\n".to_string();
    let msg = parse_raw_message(raw.as_bytes()).unwrap();
    let outcome = correlator.correlate(&msg).await.unwrap();
    assert!(matches!(
        outcome,
        CorrelationOutcome::Resolved { tier: CorrelationTier::B1Headers, .. }
    ));
}
