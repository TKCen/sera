//! Skill index system-prompt block (Hermes parity, plan area B).
//!
//! Hermes injects a stable system-prompt block that lists the skills available
//! in the runtime — descriptions only — with mandatory-load guidance: if a
//! skill matches the task, the agent MUST load its full instructions before
//! answering. SERA mirrors that contract here: scan the skills directory, parse
//! each SKILL.md (and legacy flat `*.md`) via [`sera_skills::md_loader`], and
//! render a descriptions-only block that points the agent at the `skill-view`
//! and `skill-list` tools.
//!
//! The block is intentionally body-free: it advertises availability, not
//! contents. The agent pulls full instructions on demand through `skill-view`.

use std::path::Path;

use sera_skills::discovery::discover_skills;

/// Soft cap on the rendered block size. When the listing would exceed this,
/// the tail is truncated and replaced with a `…and N more` pointer line.
const MAX_BLOCK_CHARS: usize = 4000;

/// Maximum length of a single skill description in the rendered block.
/// Longer descriptions are clamped with a trailing `…`.
const MAX_DESC_CHARS: usize = 200;

/// Build the Hermes-parity skill index system-prompt block.
///
/// Discovers skills via [`discover_skills`] (the shared single source of truth
/// for both layouts and name de-duplication). Returns `None` when the directory
/// is missing/empty or no skill parses successfully. Otherwise renders a
/// descriptions-only block sorted by skill name and capped at
/// [`MAX_BLOCK_CHARS`].
pub async fn build_skill_index_context(skills_dir: &Path) -> Option<String> {
    let skills = discover_skills(skills_dir).await;
    if skills.is_empty() {
        return None;
    }

    // discover_skills already sorts by name and dedups; map to (name, desc).
    let entries: Vec<(String, String)> = skills
        .into_iter()
        .map(|(skill, _)| (skill.name, skill.description))
        .collect();

    Some(render_block(&entries))
}

/// Sanitise a description for single-line rendering: collapse any `\n`/`\r`
/// into a space and clamp to [`MAX_DESC_CHARS`] (appending `…` when clamped).
fn sanitize_description(description: &str) -> String {
    let single_line: String = description
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    if single_line.chars().count() > MAX_DESC_CHARS {
        let clamped: String = single_line.chars().take(MAX_DESC_CHARS).collect();
        format!("{clamped}…")
    } else {
        single_line
    }
}

/// Render the descriptions-only block, truncating the list to stay under
/// [`MAX_BLOCK_CHARS`] with a `…and N more` pointer when necessary.
fn render_block(entries: &[(String, String)]) -> String {
    let header = "## Skills (mandatory)\n\
        The following skills are available in this runtime. Before answering, check whether one matches the task. If a skill matches, you MUST load its full instructions with the `skill-view` tool (pass its name); err on the side of loading. Use `skill-list` to re-check availability.\n\n";

    let mut block = String::from(header);
    for (idx, (name, description)) in entries.iter().enumerate() {
        let line = format!("- {name}: {}\n", sanitize_description(description));
        let remaining = entries.len() - idx;
        // Reserve room for a possible `…and N more` line so the cap holds even
        // after truncation.
        let pointer = format!("…and {remaining} more (use skill-list)\n");
        if block.len() + line.len() + pointer.len() > MAX_BLOCK_CHARS && idx > 0 {
            block.push_str(&pointer);
            return block;
        }
        block.push_str(&line);
    }
    block
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
    async fn renders_names_and_descriptions_from_both_layouts() {
        let dir = TempDir::new().unwrap();
        // Nested SKILL.md layout.
        write_skill_md(
            &dir.path().join("invoices/SKILL.md"),
            "lookup-invoice",
            "Find an invoice by id",
        );
        // Legacy flat layout.
        write_skill_md(&dir.path().join("greet.md"), "greeter", "Say hello");

        let block = build_skill_index_context(dir.path()).await.unwrap();

        assert!(block.contains("## Skills (mandatory)"));
        assert!(block.contains("skill-view"));
        assert!(block.contains("- greeter: Say hello"));
        assert!(block.contains("- lookup-invoice: Find an invoice by id"));
        // Body must not leak.
        assert!(!block.contains("body for"));
        // Sorted by name: greeter before lookup-invoice.
        let g = block.find("greeter").unwrap();
        let l = block.find("lookup-invoice").unwrap();
        assert!(g < l);
    }

    #[tokio::test]
    async fn missing_dir_returns_none() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(build_skill_index_context(&missing).await.is_none());
    }

    #[tokio::test]
    async fn empty_dir_returns_none() {
        let dir = TempDir::new().unwrap();
        assert!(build_skill_index_context(dir.path()).await.is_none());
    }

    #[tokio::test]
    async fn unparseable_skills_are_skipped() {
        let dir = TempDir::new().unwrap();
        // Truly unparseable: missing required `name` frontmatter → parse error, skipped.
        fs::write(
            dir.path().join("broken.md"),
            "---\ndescription: no name here\n---\nbody\n",
        )
        .unwrap();
        write_skill_md(&dir.path().join("ok.md"), "ok-skill", "works fine");

        let block = build_skill_index_context(dir.path()).await.unwrap();
        assert!(block.contains("- ok-skill: works fine"));
        // broken.md has no `name` so it is truly unparseable and must not appear.
        assert!(!block.contains("no name here"));
    }

    #[tokio::test]
    async fn description_less_skill_appears_in_index() {
        let dir = TempDir::new().unwrap();
        // Missing `description` must NOT exclude the skill from the gateway index.
        fs::write(dir.path().join("nodesc.md"), "---\nname: nodesc\n---\nbody\n").unwrap();
        write_skill_md(&dir.path().join("ok.md"), "ok-skill", "works fine");

        let block = build_skill_index_context(dir.path()).await.unwrap();
        assert!(
            block.contains("- nodesc:"),
            "description-less skill must appear in gateway index; block:\n{block}"
        );
        assert!(block.contains("- ok-skill: works fine"));
    }

    #[tokio::test]
    async fn multiline_description_renders_on_one_line() {
        let dir = TempDir::new().unwrap();
        // A description spanning multiple lines (CRLF + LF) in the YAML.
        fs::write(
            dir.path().join("multi.md"),
            "---\nname: multi\ndescription: \"line one\\nline two\\r\\nline three\"\n---\n\nbody\n",
        )
        .unwrap();

        let block = build_skill_index_context(dir.path()).await.unwrap();

        // The skill line must contain all three fragments but no embedded newline.
        let line = block
            .lines()
            .find(|l| l.starts_with("- multi:"))
            .expect("multi skill line present");
        assert!(line.contains("line one"));
        assert!(line.contains("line two"));
        assert!(line.contains("line three"));
        assert!(!line.contains('\r'), "carriage return must be sanitised");
    }

    #[tokio::test]
    async fn caps_block_with_pointer_when_many_skills() {
        let dir = TempDir::new().unwrap();
        // Many skills with long descriptions to blow past the cap.
        let long_desc = "x".repeat(200);
        for i in 0..100 {
            write_skill_md(
                &dir.path().join(format!("skill{i:03}.md")),
                &format!("skill{i:03}"),
                &long_desc,
            );
        }

        let block = build_skill_index_context(dir.path()).await.unwrap();
        assert!(block.len() <= MAX_BLOCK_CHARS, "block len = {}", block.len());
        assert!(block.contains("more (use skill-list)"));
    }
}
