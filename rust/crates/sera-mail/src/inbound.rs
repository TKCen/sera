//! Parser for inbound raw MIME messages.
//!
//! Extracts the fields the correlator needs: `Message-ID`, `In-Reply-To`,
//! `References`, `From`, `To`, `Subject`, and the body text. We intentionally
//! do not try to reconstruct attachments or multipart structure — the
//! correlator operates on pure header + body-text input.

use mailparse::{parse_mail, MailHeaderMap};

use crate::error::MailCorrelationError;

/// Parsed inbound message — the subset of an RFC 5322 message that the
/// correlator needs.
///
/// Header names are normalised to their canonical RFC capitalisation (the
/// `mailparse` crate handles this), and angle brackets around `Message-ID` /
/// `In-Reply-To` values are stripped so downstream comparisons against stored
/// thread-ids are direct equality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMessage {
    /// `Message-ID` of the inbound message itself, with angle brackets
    /// stripped. `None` if the header is absent (unusual but not fatal).
    pub message_id: Option<String>,

    /// `In-Reply-To`, angle brackets stripped. The primary correlation signal.
    pub in_reply_to: Option<String>,

    /// `References` chain, each entry with angle brackets stripped. Some
    /// clients only populate `References`, so B1 matching walks this too.
    pub references: Vec<String>,

    /// `From` address as-sent. Not used for routing; kept for logging /
    /// rate-limit keying on the B3 drop path.
    pub from: String,

    /// `To` addresses.
    pub to: Vec<String>,

    /// `Subject` line.
    pub subject: String,

    /// Decoded body text. `mailparse` concatenates text/plain parts; if the
    /// message is HTML-only the body text will be whatever the HTML decoded to
    /// (nonce extraction still works because the footer is verbatim text).
    pub body_text: String,

    /// Raw header list for debugging / auditing. Order-preserved.
    pub headers: Vec<(String, String)>,
}

/// Strip a single pair of angle brackets from `s` if present. RFC 5322
/// Message-IDs are wrapped in `<...>` on-wire; every correlator-side
/// comparison expects the unwrapped value.
fn strip_brackets(s: &str) -> String {
    let trimmed = s.trim();
    trimmed
        .strip_prefix('<')
        .and_then(|t| t.strip_suffix('>'))
        .map(|t| t.to_string())
        .unwrap_or_else(|| trimmed.to_string())
}

/// Split a `References:` header value into individual Message-IDs.
///
/// RFC 5322 says References is a whitespace-separated list of `msg-id`
/// tokens, each bracketed. We tolerate stray whitespace / commas.
fn split_references(raw: &str) -> Vec<String> {
    raw.split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .map(strip_brackets)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a raw MIME blob into an [`InboundMessage`].
///
/// Returns [`MailCorrelationError::ParseFailed`] if `mailparse` cannot tokenise
/// the input (grossly malformed header block, invalid content-transfer-encoding,
/// etc.).
pub fn parse_raw_message(raw: &[u8]) -> Result<InboundMessage, MailCorrelationError> {
    let parsed = parse_mail(raw).map_err(|e| MailCorrelationError::ParseFailed(e.to_string()))?;

    let headers = &parsed.headers;

    let message_id = headers.get_first_value("Message-ID").map(|s| strip_brackets(&s));
    let in_reply_to = headers.get_first_value("In-Reply-To").map(|s| strip_brackets(&s));
    let references = headers
        .get_first_value("References")
        .map(|s| split_references(&s))
        .unwrap_or_default();

    let from = headers.get_first_value("From").unwrap_or_default();
    let to = headers
        .get_first_value("To")
        .map(|s| {
            s.split(',')
                .map(|addr| addr.trim().to_string())
                .filter(|a| !a.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let subject = headers.get_first_value("Subject").unwrap_or_default();

    // Body: concatenate text/plain subparts, falling back to the root body.
    let body_text = if parsed.subparts.is_empty() {
        parsed
            .get_body()
            .map_err(|e| MailCorrelationError::ParseFailed(e.to_string()))?
    } else {
        let mut acc = String::new();
        for part in &parsed.subparts {
            let ctype = part.ctype.mimetype.to_ascii_lowercase();
            if (ctype == "text/plain" || ctype.is_empty())
                && let Ok(text) = part.get_body()
            {
                if !acc.is_empty() {
                    acc.push('\n');
                }
                acc.push_str(&text);
            }
        }
        if acc.is_empty() {
            // No text/plain subpart — fall back to whatever the root yields
            // (likely HTML collapsed into text). Nonce extraction is still
            // correct because the footer is verbatim ASCII.
            parsed
                .get_body()
                .map_err(|e| MailCorrelationError::ParseFailed(e.to_string()))?
        } else {
            acc
        }
    };

    let raw_headers: Vec<(String, String)> = headers
        .iter()
        .map(|h| (h.get_key(), h.get_value()))
        .collect();

    Ok(InboundMessage {
        message_id,
        in_reply_to,
        references,
        from,
        to,
        subject,
        body_text,
        headers: raw_headers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GMAIL_REPLY: &[u8] = b"\
From: alice@gmail.com\r\n\
To: sera@example.com\r\n\
Subject: Re: Gate ping\r\n\
Message-ID: <CAK123reply@mail.gmail.com>\r\n\
In-Reply-To: <sera-gate-abc@sera.local>\r\n\
References: <sera-gate-abc@sera.local> <CAK123prev@mail.gmail.com>\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Yes, approved.\r\n\
\r\n\
> On Mon, SERA wrote:\r\n\
> [SERA:nonce=AAAABBBBCCCCDDDD]\r\n";

    #[test]
    fn gmail_reply_parses_all_headers() {
        let msg = parse_raw_message(GMAIL_REPLY).unwrap();
        assert_eq!(msg.message_id.as_deref(), Some("CAK123reply@mail.gmail.com"));
        assert_eq!(msg.in_reply_to.as_deref(), Some("sera-gate-abc@sera.local"));
        assert_eq!(msg.references.len(), 2);
        assert_eq!(msg.references[0], "sera-gate-abc@sera.local");
        assert_eq!(msg.references[1], "CAK123prev@mail.gmail.com");
        assert_eq!(msg.from, "alice@gmail.com");
        assert_eq!(msg.to, vec!["sera@example.com"]);
        assert_eq!(msg.subject, "Re: Gate ping");
        assert!(msg.body_text.contains("Yes, approved."));
        assert!(msg.body_text.contains("[SERA:nonce=AAAABBBBCCCCDDDD]"));
    }

    const OUTLOOK_REPLY: &[u8] = b"\
from: bob@outlook.com\r\n\
to: sera@example.com\r\n\
subject: RE: Approval needed\r\n\
message-id: <DM6PR01MB-reply@outlook>\r\n\
in-reply-to: <sera-gate-xyz@sera.local>\r\n\
content-type: text/plain\r\n\
\r\n\
Looks good.\r\n";

    #[test]
    fn outlook_reply_case_insensitive_headers() {
        let msg = parse_raw_message(OUTLOOK_REPLY).unwrap();
        assert_eq!(msg.message_id.as_deref(), Some("DM6PR01MB-reply@outlook"));
        assert_eq!(msg.in_reply_to.as_deref(), Some("sera-gate-xyz@sera.local"));
        assert!(msg.references.is_empty());
        assert_eq!(msg.from, "bob@outlook.com");
    }

    const FORWARDED_NO_IRT: &[u8] = b"\
From: carol@example.com\r\n\
To: sera@example.com\r\n\
Subject: Fwd: Gate ping\r\n\
Message-ID: <fwd-123@example.com>\r\n\
Content-Type: text/plain\r\n\
\r\n\
FYI, see below.\r\n\
\r\n\
---------- Forwarded message ----------\r\n\
From: SERA <sera@example.com>\r\n\
Subject: Gate ping\r\n\
\r\n\
Please approve.\r\n\
[SERA:nonce=FWDNONCE12345678]\r\n";

    #[test]
    fn forwarded_message_no_headers_keeps_nonce_in_body() {
        let msg = parse_raw_message(FORWARDED_NO_IRT).unwrap();
        assert!(msg.in_reply_to.is_none());
        assert!(msg.references.is_empty());
        assert!(msg.body_text.contains("[SERA:nonce=FWDNONCE12345678]"));
    }

    const HEADER_STRIPPED: &[u8] = b"\
From: dave@example.com\r\n\
To: sera@example.com\r\n\
Subject: Re: Ping\r\n\
Content-Type: text/plain\r\n\
\r\n\
Approve.\r\n\
[SERA:nonce=ABCDEFGHIJK123456]\r\n";

    #[test]
    fn header_stripped_reply_still_has_body_nonce() {
        let msg = parse_raw_message(HEADER_STRIPPED).unwrap();
        assert!(msg.message_id.is_none());
        assert!(msg.in_reply_to.is_none());
        assert!(msg.body_text.contains("[SERA:nonce=ABCDEFGHIJK123456]"));
    }

    const MULTIPART_ALTERNATIVE: &[u8] = b"\
From: eve@example.com\r\n\
To: sera@example.com\r\n\
Subject: Re: Multipart\r\n\
Message-ID: <mp-1@example.com>\r\n\
In-Reply-To: <sera-gate-mp@sera.local>\r\n\
MIME-Version: 1.0\r\n\
Content-Type: multipart/alternative; boundary=\"BOUND\"\r\n\
\r\n\
--BOUND\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
\r\n\
Approved.\r\n\
[SERA:nonce=MPNONCE1234]\r\n\
--BOUND\r\n\
Content-Type: text/html; charset=utf-8\r\n\
\r\n\
<p>Approved.</p>\r\n\
--BOUND--\r\n";

    #[test]
    fn multipart_alternative_extracts_text_plain() {
        let msg = parse_raw_message(MULTIPART_ALTERNATIVE).unwrap();
        assert_eq!(msg.in_reply_to.as_deref(), Some("sera-gate-mp@sera.local"));
        assert!(msg.body_text.contains("Approved."));
        assert!(msg.body_text.contains("[SERA:nonce=MPNONCE1234]"));
    }

    #[test]
    fn malformed_header_continuation_errors() {
        // A header line that begins with whitespace but no prior header is a
        // parse error — it is a continuation of a header that does not exist.
        // mailparse rejects this as malformed input.
        let bad = b" orphan continuation line\r\nFrom: a@b\r\n\r\nbody\r\n";
        let err = parse_raw_message(bad).unwrap_err();
        assert!(matches!(err, MailCorrelationError::ParseFailed(_)));
    }

    #[test]
    fn empty_buffer_parses_to_empty_message() {
        // mailparse is lenient on empty input; document the behaviour so
        // downstream correlator tests rely on B3 rather than a parse failure.
        let msg = parse_raw_message(b"").unwrap();
        assert!(msg.message_id.is_none());
        assert!(msg.in_reply_to.is_none());
        assert!(msg.body_text.is_empty());
    }

    #[test]
    fn split_references_tolerates_whitespace_and_commas() {
        let refs =
            split_references("  <a@b>  <c@d>,<e@f>\t<g@h>");
        assert_eq!(refs, vec!["a@b", "c@d", "e@f", "g@h"]);
    }

    #[test]
    fn strip_brackets_passes_through_unbracketed() {
        assert_eq!(strip_brackets("<a@b>"), "a@b");
        assert_eq!(strip_brackets("a@b"), "a@b");
        assert_eq!(strip_brackets("  <x@y>  "), "x@y");
    }
}
