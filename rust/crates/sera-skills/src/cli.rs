//! CLI helpers — formatting routines for a future `sera skills` subcommand.
//!
//! SERA does not yet expose a unified top-level Rust CLI binary. We keep
//! the formatting logic here so it is trivial to wire into whichever binary
//! wins that turf war (see the follow-up bead noted in the phase-5 handoff).
//!
//! # Follow-up
//!
//! > Wire `sera skills search` into `sera-runtime` (or a new `sera` CLI
//! > crate) once the top-level CLI crate decision is made.

use crate::resolver::SkillResolver;

/// Run a search through the given resolver and return a human-readable,
/// 3-column ASCII table (name, version, source).
///
/// The first line is a header; each following line is a hit. An empty
/// resolver or zero-hit query produces a header-only table and a trailing
/// `(no results)` note, so the output is never ambiguous.
pub async fn run_search(query: &str, resolver: &SkillResolver) -> String {
    let hits = resolver.search(query).await;
    let mut rows: Vec<[String; 3]> = Vec::with_capacity(hits.len() + 1);
    rows.push(["NAME".into(), "VERSION".into(), "SOURCE".into()]);
    for h in &hits {
        let version = if h.version.is_empty() {
            "-".to_string()
        } else {
            h.version.clone()
        };
        rows.push([h.name.clone(), version, h.source.to_string()]);
    }

    let widths = column_widths(&rows);
    let mut out = String::new();
    for row in &rows {
        out.push_str(&format!(
            "{:<w0$}  {:<w1$}  {:<w2$}\n",
            row[0],
            row[1],
            row[2],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2],
        ));
    }
    if hits.is_empty() {
        out.push_str("(no results)\n");
    }
    out
}

fn column_widths(rows: &[[String; 3]]) -> [usize; 3] {
    let mut w = [0usize; 3];
    for row in rows {
        for i in 0..3 {
            w[i] = w[i].max(row[i].len());
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_ref::{SkillRef, SkillSourceKind};
    use crate::source::{ResolvedSkill, SkillSearchHit, SkillSource};
    use async_trait::async_trait;
    use std::sync::Arc;

    struct StaticSource {
        kind: SkillSourceKind,
        hits: Vec<SkillSearchHit>,
    }

    #[async_trait]
    impl SkillSource for StaticSource {
        fn kind(&self) -> SkillSourceKind {
            self.kind
        }
        async fn resolve(&self, _r: &SkillRef) -> Result<ResolvedSkill, crate::error::SkillsError> {
            unreachable!()
        }
        async fn search(&self, _q: &str) -> Result<Vec<SkillSearchHit>, crate::error::SkillsError> {
            Ok(self.hits.clone())
        }
    }

    #[tokio::test]
    async fn run_search_formats_header_only_for_empty() {
        let resolver = SkillResolver::new(vec![]);
        let out = run_search("anything", &resolver).await;
        assert!(out.contains("NAME"));
        assert!(out.contains("(no results)"));
    }

    #[tokio::test]
    async fn run_search_renders_hits() {
        let src = StaticSource {
            kind: SkillSourceKind::Fs,
            hits: vec![SkillSearchHit {
                name: "triage".into(),
                version: "1.0.0".into(),
                description: "d".into(),
                source: SkillSourceKind::Fs,
                pack_name: "p".into(),
            }],
        };
        let resolver = SkillResolver::new(vec![Arc::new(src)]);
        let out = run_search("", &resolver).await;
        assert!(out.contains("triage"));
        assert!(out.contains("1.0.0"));
        assert!(out.contains("fs"));
    }

    #[tokio::test]
    async fn run_search_substitutes_dash_for_missing_version() {
        let src = StaticSource {
            kind: SkillSourceKind::Fs,
            hits: vec![SkillSearchHit {
                name: "nv".into(),
                version: String::new(),
                description: "d".into(),
                source: SkillSourceKind::Fs,
                pack_name: "p".into(),
            }],
        };
        let resolver = SkillResolver::new(vec![Arc::new(src)]);
        let out = run_search("", &resolver).await;
        assert!(out.contains(" - "));
    }
}
