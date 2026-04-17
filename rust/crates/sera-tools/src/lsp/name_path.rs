//! Name-path parser for LSP symbol queries.
//!
//! Phase 2 scope — see `docs/plan/LSP-TOOLS-DESIGN.md` §3.2.
//!
//! # Syntax
//!
//! * `Tool` — one segment, relative.
//! * `ToolRegistry/get` — two segments, relative (scoped by their textual
//!   ancestry).
//! * `/MyStruct/new` — absolute: rooted at file scope (no outer container).
//! * `method[0]` — overload index on a segment. The index is a non-negative
//!   `u32` literal; anything else inside the brackets rejects.
//!
//! Empty segments reject. The leading `/` toggles `absolute` but is not itself
//! a segment.

use super::error::LspError;

/// A parsed name-path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamePath {
    pub segments: Vec<NamePathSegment>,
    /// `true` when the raw pattern begins with `/` — pins the match to a
    /// file-scope root, disallowing outer containers.
    pub absolute: bool,
}

/// One path segment: a name with an optional overload index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamePathSegment {
    pub name: String,
    /// `method[0]` → `Some(0)`; `method` → `None`.
    pub overload_index: Option<u32>,
}

impl NamePath {
    /// Parse a raw name-path pattern. Returns [`LspError::InvalidNamePath`] on
    /// empty segments, unterminated brackets, or non-integer overload indices.
    pub fn parse(raw: &str) -> Result<NamePath, LspError> {
        let (absolute, body) = if let Some(rest) = raw.strip_prefix('/') {
            (true, rest)
        } else {
            (false, raw)
        };

        if body.is_empty() {
            return Err(LspError::InvalidNamePath {
                raw: raw.to_string(),
                reason: "empty path".into(),
            });
        }

        let mut segments = Vec::new();
        for raw_seg in body.split('/') {
            let seg = Self::parse_segment(raw_seg, raw)?;
            segments.push(seg);
        }

        Ok(NamePath { segments, absolute })
    }

    fn parse_segment(raw_seg: &str, full: &str) -> Result<NamePathSegment, LspError> {
        if raw_seg.is_empty() {
            return Err(LspError::InvalidNamePath {
                raw: full.to_string(),
                reason: "empty segment".into(),
            });
        }
        match raw_seg.find('[') {
            None => {
                if raw_seg.contains(']') {
                    return Err(LspError::InvalidNamePath {
                        raw: full.to_string(),
                        reason: format!("stray `]` in segment `{raw_seg}`"),
                    });
                }
                Ok(NamePathSegment {
                    name: raw_seg.to_string(),
                    overload_index: None,
                })
            }
            Some(br) => {
                let name = &raw_seg[..br];
                let rest = &raw_seg[br..];
                if name.is_empty() {
                    return Err(LspError::InvalidNamePath {
                        raw: full.to_string(),
                        reason: "missing name before `[`".into(),
                    });
                }
                if !rest.ends_with(']') {
                    return Err(LspError::InvalidNamePath {
                        raw: full.to_string(),
                        reason: format!("unterminated overload bracket in `{raw_seg}`"),
                    });
                }
                let inside = &rest[1..rest.len() - 1];
                // Only a non-negative u32 literal is allowed; forbid `-`, `+`,
                // whitespace, and any other non-digit characters.
                if inside.is_empty() || !inside.chars().all(|c| c.is_ascii_digit()) {
                    return Err(LspError::InvalidNamePath {
                        raw: full.to_string(),
                        reason: format!("invalid overload index `[{inside}]`"),
                    });
                }
                let idx = inside.parse::<u32>().map_err(|e| LspError::InvalidNamePath {
                    raw: full.to_string(),
                    reason: format!("overload index `[{inside}]`: {e}"),
                })?;
                Ok(NamePathSegment {
                    name: name.to_string(),
                    overload_index: Some(idx),
                })
            }
        }
    }

    /// Round-trip the parsed path back to its canonical string form.
    /// `parse(format(p))` must equal `p` for any parsed `p`.
    pub fn format(&self) -> String {
        let mut out = String::new();
        if self.absolute {
            out.push('/');
        }
        for (i, seg) in self.segments.iter().enumerate() {
            if i > 0 {
                out.push('/');
            }
            out.push_str(&seg.name);
            if let Some(ix) = seg.overload_index {
                out.push('[');
                out.push_str(&ix.to_string());
                out.push(']');
            }
        }
        out
    }

    /// Return the last segment's bare name — used as the `query` argument for
    /// `workspace/symbol`.
    pub fn last_segment_name(&self) -> &str {
        self.segments
            .last()
            .map(|s| s.name.as_str())
            .unwrap_or("")
    }

    /// Match a candidate `other` against `self`.
    ///
    /// * When `substring` is `true` **and** the pattern is a single segment
    ///   without an overload index, the match succeeds when the candidate's
    ///   last segment name contains the pattern's name as a substring. This
    ///   mirrors the design-doc behaviour for short patterns like `"get"`.
    /// * Otherwise every pattern segment must equal the corresponding
    ///   candidate segment **tail-aligned** when `!self.absolute`, and
    ///   **head-aligned** when `self.absolute`.
    /// * Overload indices, when present in the pattern, must match exactly.
    pub fn matches(&self, other: &NamePath, substring: bool) -> bool {
        // Single-segment substring fast-path.
        if substring
            && self.segments.len() == 1
            && self.segments[0].overload_index.is_none()
            && !self.absolute
        {
            let needle = &self.segments[0].name;
            if let Some(last) = other.segments.last() {
                return last.name.contains(needle);
            }
            return false;
        }

        // Absolute: head-aligned equality.
        if self.absolute {
            if self.segments.len() != other.segments.len() {
                return false;
            }
            return self
                .segments
                .iter()
                .zip(other.segments.iter())
                .all(|(a, b)| segment_eq(a, b));
        }

        // Relative: tail-aligned equality. The pattern's segments must match
        // the suffix of `other.segments`.
        if self.segments.len() > other.segments.len() {
            return false;
        }
        let offset = other.segments.len() - self.segments.len();
        self.segments
            .iter()
            .enumerate()
            .all(|(i, pseg)| segment_eq(pseg, &other.segments[offset + i]))
    }
}

fn segment_eq(pattern: &NamePathSegment, candidate: &NamePathSegment) -> bool {
    if pattern.name != candidate.name {
        return false;
    }
    match pattern.overload_index {
        Some(p) => candidate.overload_index == Some(p),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::collection;
    use proptest::prelude::*;

    #[test]
    fn parse_single_segment() {
        let np = NamePath::parse("Tool").unwrap();
        assert!(!np.absolute);
        assert_eq!(np.segments.len(), 1);
        assert_eq!(np.segments[0].name, "Tool");
        assert!(np.segments[0].overload_index.is_none());
    }

    #[test]
    fn parse_two_segments_relative() {
        let np = NamePath::parse("ToolRegistry/get").unwrap();
        assert!(!np.absolute);
        assert_eq!(np.segments.len(), 2);
        assert_eq!(np.segments[1].name, "get");
    }

    #[test]
    fn parse_absolute_two_segments() {
        let np = NamePath::parse("/MyStruct/new").unwrap();
        assert!(np.absolute);
        assert_eq!(np.segments.len(), 2);
        assert_eq!(np.segments[0].name, "MyStruct");
        assert_eq!(np.segments[1].name, "new");
    }

    #[test]
    fn parse_overload_index() {
        let np = NamePath::parse("method[0]").unwrap();
        assert_eq!(np.segments[0].name, "method");
        assert_eq!(np.segments[0].overload_index, Some(0));
    }

    #[test]
    fn parse_rejects_empty() {
        let err = NamePath::parse("").unwrap_err();
        assert!(matches!(err, LspError::InvalidNamePath { .. }));
        let err = NamePath::parse("/").unwrap_err();
        assert!(matches!(err, LspError::InvalidNamePath { .. }));
    }

    #[test]
    fn parse_rejects_double_slash() {
        let err = NamePath::parse("foo//bar").unwrap_err();
        assert!(matches!(err, LspError::InvalidNamePath { .. }));
    }

    #[test]
    fn parse_rejects_trailing_slash() {
        let err = NamePath::parse("foo/").unwrap_err();
        assert!(matches!(err, LspError::InvalidNamePath { .. }));
    }

    #[test]
    fn parse_rejects_bad_overload_index() {
        let cases = &["m[-1]", "m[a]", "m[]", "m[1", "m1]", "m[1 2]", "m[ 1]"];
        for bad in cases {
            let err = NamePath::parse(bad).unwrap_err();
            assert!(
                matches!(err, LspError::InvalidNamePath { .. }),
                "expected invalid: {bad}"
            );
        }
    }

    #[test]
    fn format_round_trip_examples() {
        for raw in ["Tool", "ToolRegistry/get", "/MyStruct/new", "method[0]"] {
            let np = NamePath::parse(raw).unwrap();
            assert_eq!(np.format(), raw, "round-trip failed for {raw}");
            let np2 = NamePath::parse(&np.format()).unwrap();
            assert_eq!(np, np2);
        }
    }

    #[test]
    fn matches_substring_single_segment() {
        let pat = NamePath::parse("get").unwrap();
        let cand = NamePath::parse("ToolRegistry/get_or_spawn").unwrap();
        assert!(pat.matches(&cand, true));
        assert!(!pat.matches(&cand, false));
    }

    #[test]
    fn matches_exact_multi_segment_relative() {
        let pat = NamePath::parse("ToolRegistry/get").unwrap();
        let cand = NamePath::parse("outer/ToolRegistry/get").unwrap();
        assert!(pat.matches(&cand, false));
        // Head-only match — does not count.
        let cand2 = NamePath::parse("ToolRegistry/get_extra").unwrap();
        assert!(!pat.matches(&cand2, false));
    }

    #[test]
    fn matches_absolute_requires_full_path() {
        let pat = NamePath::parse("/MyStruct/new").unwrap();
        let exact = NamePath::parse("/MyStruct/new").unwrap();
        let too_deep = NamePath::parse("/outer/MyStruct/new").unwrap();
        // `matches` is called with `other` being a candidate — absolute-ness
        // on the candidate is informational; length must match.
        assert!(pat.matches(&exact, false));
        assert!(!pat.matches(&too_deep, false));
    }

    #[test]
    fn matches_respects_overload_index() {
        let pat = NamePath::parse("method[0]").unwrap();
        let same = NamePath::parse("method[0]").unwrap();
        let diff = NamePath::parse("method[1]").unwrap();
        let bare = NamePath::parse("method").unwrap();
        assert!(pat.matches(&same, false));
        assert!(!pat.matches(&diff, false));
        assert!(!pat.matches(&bare, false));
        // But bare pattern accepts any overload.
        assert!(bare.matches(&same, false));
    }

    // -----------------------------------------------------------------------
    // Proptest — see design §12.2.
    // -----------------------------------------------------------------------

    /// Strategy for one valid segment: ASCII alnum + underscore, optional
    /// `[<u32>]` suffix. Restricted to the characters the parser accepts.
    fn segment_strategy() -> impl Strategy<Value = String> {
        (
            "[A-Za-z_][A-Za-z0-9_]{0,8}",
            proptest::option::of(0u32..=999),
        )
            .prop_map(|(name, idx)| match idx {
                Some(i) => format!("{name}[{i}]"),
                None => name,
            })
    }

    /// Strategy for a valid name-path: 1..4 segments, optional leading `/`.
    fn name_path_strategy() -> impl Strategy<Value = String> {
        (
            any::<bool>(),
            collection::vec(segment_strategy(), 1..=4),
        )
            .prop_map(|(abs, segs)| {
                let body = segs.join("/");
                if abs {
                    format!("/{body}")
                } else {
                    body
                }
            })
    }

    proptest! {
        #[test]
        fn roundtrip_parse_format(raw in name_path_strategy()) {
            let parsed = NamePath::parse(&raw).expect("valid by construction");
            let formatted = parsed.format();
            let re_parsed = NamePath::parse(&formatted).expect("re-parse");
            prop_assert_eq!(parsed, re_parsed);
            prop_assert_eq!(formatted, raw);
        }

        /// Arbitrary Unicode input must never panic the parser.
        #[test]
        fn no_panic_on_arbitrary_unicode(raw in any::<String>()) {
            let _ = NamePath::parse(&raw);
        }

        /// Any bracket content that is not a bare u32 literal must reject.
        #[test]
        fn overload_only_accepts_u32(
            name in "[A-Za-z_][A-Za-z0-9_]{0,6}",
            junk in "[^0-9\\]]{1,5}",
        ) {
            // Insert junk into the brackets; parser must reject.
            let raw = format!("{name}[{junk}]");
            let res = NamePath::parse(&raw);
            prop_assert!(res.is_err(), "should reject `{raw}`");
        }
    }
}
