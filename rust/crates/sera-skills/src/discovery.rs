//! Skill discovery — single source of truth for locating SKILL.md files.
//!
//! Shared by the gateway's system-prompt index block and the resident
//! `skills_list` / `skill_view` tools. Divergence here previously let the
//! index advertise skills the tools couldn't resolve (the index scanned
//! recursively and deduped by name, while the tools scanned one level and
//! didn't), so name collisions returned arbitrary bodies. Both call sites now
//! route through [`discover_skills`].
//!
//! Two layouts are supported:
//! - **Hermes layout**: `SKILL.md` files found recursively up to [`MAX_DEPTH`].
//! - **Flat layout**: top-level `*.md` files directly under the skills dir
//!   (case-insensitive `md` extension).

use std::path::{Path, PathBuf};

use tracing::warn;

use crate::md_loader::{load_skill_md, Skill};

/// Maximum directory recursion depth when scanning for `SKILL.md` files.
pub const MAX_DEPTH: usize = 3;

/// Discover all skills under `skills_dir`.
///
/// Collects (a) `SKILL.md` files recursively up to [`MAX_DEPTH`] and (b)
/// top-level `*.md` files (legacy flat layout). File paths are de-duplicated
/// first (a top-level `SKILL.md` matches both scans). Each file is parsed via
/// [`load_skill_md`]; parse failures emit a `warn!` and are skipped.
///
/// Results are sorted by `(name, path)` for determinism, then de-duplicated by
/// name keeping the **first** entry — each collision emits a `warn!` naming
/// both paths.
///
/// Returns `Vec<(Skill, skill_file_path)>`.
pub async fn discover_skills(skills_dir: &Path) -> Vec<(Skill, PathBuf)> {
    let mut paths: Vec<PathBuf> = Vec::new();
    collect_skill_md(skills_dir, 0, &mut paths);
    collect_flat_md(skills_dir, &mut paths);

    // De-duplicate file paths (a top-level file literally named SKILL.md
    // matches both scans).
    paths.sort();
    paths.dedup();

    let mut results: Vec<(Skill, PathBuf)> = Vec::new();
    for path in paths {
        match load_skill_md(&path).await {
            Ok(skill) => results.push((skill, path)),
            Err(err) => warn!(
                path = %path.display(),
                error = %err,
                "skill discovery: skipping unparseable skill"
            ),
        }
    }

    // Sort by (name, path) for deterministic collision resolution.
    results.sort_by(|(a, ap), (b, bp)| a.name.cmp(&b.name).then_with(|| ap.cmp(bp)));

    // De-duplicate by name, keeping the first entry; warn on each collision.
    let mut deduped: Vec<(Skill, PathBuf)> = Vec::with_capacity(results.len());
    for (skill, path) in results {
        if let Some((kept, kept_path)) = deduped.last()
            && kept.name == skill.name
        {
            warn!(
                name = %skill.name,
                kept = %kept_path.display(),
                dropped = %path.display(),
                "skill discovery: duplicate skill name, keeping first"
            );
            continue;
        }
        deduped.push((skill, path));
    }

    deduped
}

/// Recursively collect `SKILL.md` files under `dir` up to [`MAX_DEPTH`].
fn collect_skill_md(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_skill_md(&path, depth + 1, out);
        } else if path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md") {
            out.push(path);
        }
    }
}

/// Collect top-level `*.md` files directly under `dir` (legacy flat layout).
fn collect_flat_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_skill_md(path: &Path, name: &str, description: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(
            path,
            format!("---\nname: {name}\ndescription: {description}\n---\n\nbody for {name}\n"),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn finds_hermes_and_flat_layouts() {
        let dir = TempDir::new().unwrap();
        write_skill_md(
            &dir.path().join("invoices/SKILL.md"),
            "lookup-invoice",
            "Find an invoice by id",
        );
        write_skill_md(&dir.path().join("greet.md"), "greeter", "Say hello");

        let skills = discover_skills(dir.path()).await;
        let names: Vec<&str> = skills.iter().map(|(s, _)| s.name.as_str()).collect();
        assert!(names.contains(&"lookup-invoice"), "missing hermes skill");
        assert!(names.contains(&"greeter"), "missing flat skill");
        // Sorted by name: greeter < lookup-invoice.
        assert_eq!(names, vec!["greeter", "lookup-invoice"]);
    }

    #[tokio::test]
    async fn depth_two_nested_skill_is_found() {
        let dir = TempDir::new().unwrap();
        // group/research/SKILL.md — depth 2 (group=1, research=2), within MAX_DEPTH.
        write_skill_md(
            &dir.path().join("group/research/SKILL.md"),
            "research",
            "Nested research skill",
        );

        let skills = discover_skills(dir.path()).await;
        let names: Vec<&str> = skills.iter().map(|(s, _)| s.name.as_str()).collect();
        assert!(names.contains(&"research"), "depth-2 nested skill not found");
    }

    #[tokio::test]
    async fn depth_beyond_max_is_not_found() {
        let dir = TempDir::new().unwrap();
        // a/b/c/d/SKILL.md — SKILL.md sits at depth 4, beyond MAX_DEPTH=3.
        write_skill_md(
            &dir.path().join("a/b/c/d/SKILL.md"),
            "too-deep",
            "Should not be discovered",
        );

        let skills = discover_skills(dir.path()).await;
        let names: Vec<&str> = skills.iter().map(|(s, _)| s.name.as_str()).collect();
        assert!(
            !names.contains(&"too-deep"),
            "skill beyond MAX_DEPTH should not be found, got: {names:?}"
        );
    }

    #[tokio::test]
    async fn name_collision_yields_one_deterministic_entry() {
        let dir = TempDir::new().unwrap();
        // Both resolve to name `foo`: a flat `foo.md` and a `foo/SKILL.md`.
        write_skill_md(&dir.path().join("foo.md"), "foo", "flat foo");
        write_skill_md(&dir.path().join("foo/SKILL.md"), "foo", "hermes foo");

        let skills = discover_skills(dir.path()).await;
        let foos: Vec<&(Skill, PathBuf)> =
            skills.iter().filter(|(s, _)| s.name == "foo").collect();
        assert_eq!(foos.len(), 1, "collision should yield exactly one entry");

        // Determinism: sorted by (name, path), first path wins. `Path` ordering
        // compares component-by-component, so `<dir>/foo/SKILL.md` vs
        // `<dir>/foo.md` is decided by the first differing component: the dir
        // component `foo` is a prefix of the file component `foo.md`, so `foo`
        // sorts first and `foo/SKILL.md` is kept every run.
        let kept = &foos[0].1;
        assert_eq!(
            kept,
            &dir.path().join("foo").join("SKILL.md"),
            "collision must deterministically keep foo/SKILL.md"
        );
        assert_eq!(foos[0].0.description, "hermes foo");
    }

    #[tokio::test]
    async fn unparseable_file_is_skipped() {
        let dir = TempDir::new().unwrap();
        // Truly unparseable: missing required `name` → parse error, skipped.
        fs::write(dir.path().join("broken.md"), "---\ndescription: no name here\n---\nbody\n")
            .unwrap();
        write_skill_md(&dir.path().join("ok.md"), "ok-skill", "works fine");

        let skills = discover_skills(dir.path()).await;
        let names: Vec<&str> = skills.iter().map(|(s, _)| s.name.as_str()).collect();
        assert!(names.contains(&"ok-skill"), "valid skill should be present");
        // broken.md has no `name`, so it should still be skipped.
        assert!(
            !skills.iter().any(|(s, _)| s.description == "no name here"),
            "truly unparseable skill should be skipped"
        );
    }

    #[tokio::test]
    async fn description_less_skill_is_preserved() {
        let dir = TempDir::new().unwrap();
        // Missing `description` must NOT drop the skill — description defaults to "".
        fs::write(dir.path().join("nodesc.md"), "---\nname: nodesc\n---\nbody\n").unwrap();
        write_skill_md(&dir.path().join("ok.md"), "ok-skill", "works fine");

        let skills = discover_skills(dir.path()).await;
        let names: Vec<&str> = skills.iter().map(|(s, _)| s.name.as_str()).collect();
        assert!(
            names.contains(&"nodesc"),
            "description-less skill must be preserved, got: {names:?}"
        );
        let nodesc = skills.iter().find(|(s, _)| s.name == "nodesc").unwrap();
        assert_eq!(
            nodesc.0.description, "",
            "missing description must default to empty string"
        );
    }
}
