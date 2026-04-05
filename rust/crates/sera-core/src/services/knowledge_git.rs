//! Knowledge Git Service — versioned knowledge storage using git.
//!
//! Uses `git` CLI via `std::process::Command` to manage per-agent knowledge repositories.
//! Each agent gets a subdirectory in the knowledge base; repositories are initialized on first use.
//! Commits are tagged with `agent_{agent_id}_v{N}` for easy version tracking.

use std::path::PathBuf;
use std::process::Command;
use thiserror::Error;

/// Error types for knowledge git operations.
#[derive(Debug, Error)]
pub enum KnowledgeGitError {
    #[error("Git command failed: {0}")]
    GitCommand(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Agent repository not found: {0}")]
    NotFound(String),

    #[error("Invalid UTF-8 in git output")]
    InvalidUtf8,
}

/// Represents a versioned knowledge commit.
#[derive(Debug, Clone)]
pub struct KnowledgeVersion {
    pub commit_hash: String,
    pub message: String,
    pub timestamp: String,
    pub agent_id: String,
}

/// CLI-based knowledge store using git via Command.
pub struct GitCliKnowledgeStore {
    base_dir: PathBuf,
}

impl GitCliKnowledgeStore {
    /// Create a new git-based knowledge store.
    /// `base_dir` should be `{SERA_DATA_DIR}/knowledge`.
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Get the repository path for an agent.
    fn agent_repo_path(&self, agent_id: &str) -> PathBuf {
        self.base_dir.join(agent_id)
    }

    /// Initialize git repo for an agent if it doesn't exist.
    async fn ensure_repo(&self, agent_id: &str) -> Result<(), KnowledgeGitError> {
        let repo_path = self.agent_repo_path(agent_id);

        // Check if repo already exists
        if repo_path.join(".git").exists() {
            return Ok(());
        }

        // Create parent directories
        tokio::fs::create_dir_all(&repo_path)
            .await
            .map_err(KnowledgeGitError::Io)?;

        // Initialize git repo
        let output = Command::new("git")
            .arg("init")
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(KnowledgeGitError::GitCommand(format!(
                "Failed to init repo: {}",
                stderr
            )));
        }

        // Configure git user for commits
        Command::new("git")
            .args(["config", "user.email", "sera-agent@local"])
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        Command::new("git")
            .args(["config", "user.name", "SERA Agent"])
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        Ok(())
    }

    /// Get the version count for an agent (for tag numbering).
    async fn version_count(&self, agent_id: &str) -> Result<usize, KnowledgeGitError> {
        let repo_path = self.agent_repo_path(agent_id);

        let output = Command::new("git")
            .args(["tag", "-l", &format!("agent_{}_v*", agent_id)])
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        if !output.status.success() {
            return Ok(0);
        }

        let tags = String::from_utf8_lossy(&output.stdout);
        let count = tags.lines().count();
        Ok(count)
    }

    /// Commit a new version of agent knowledge.
    /// Returns the commit hash.
    pub async fn commit_version(
        &self,
        agent_id: &str,
        content: &str,
        message: &str,
    ) -> Result<String, KnowledgeGitError> {
        self.ensure_repo(agent_id).await?;

        let repo_path = self.agent_repo_path(agent_id);
        let knowledge_file = repo_path.join("knowledge.md");

        // Write the content to the knowledge file
        tokio::fs::write(&knowledge_file, content)
            .await
            .map_err(KnowledgeGitError::Io)?;

        // Stage the file
        let output = Command::new("git")
            .args(["add", "knowledge.md"])
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(KnowledgeGitError::GitCommand(format!(
                "Failed to stage file: {}",
                stderr
            )));
        }

        // Commit the change
        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(KnowledgeGitError::GitCommand(format!(
                "Failed to commit: {}",
                stderr
            )));
        }

        // Get the commit hash
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(KnowledgeGitError::GitCommand(format!(
                "Failed to get commit hash: {}",
                stderr
            )));
        }

        let commit_hash = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        // Tag the commit
        let version_count = self.version_count(agent_id).await?;
        let tag = format!("agent_{}_v{}", agent_id, version_count + 1);

        let output = Command::new("git")
            .args(["tag", &tag, &commit_hash])
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(KnowledgeGitError::GitCommand(format!(
                "Failed to create tag: {}",
                stderr
            )));
        }

        Ok(commit_hash)
    }

    /// List all versions for an agent, newest first.
    pub async fn list_versions(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<KnowledgeVersion>, KnowledgeGitError> {
        self.ensure_repo(agent_id).await?;

        let repo_path = self.agent_repo_path(agent_id);

        // Get commit log with timestamps
        let output = Command::new("git")
            .args([
                "log",
                "--format=%H%n%s%n%aI",
                &format!("--max-count={}", limit),
            ])
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        if !output.status.success() {
            // Repo might be empty
            return Ok(Vec::new());
        }

        let log = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = log.lines().collect();

        let mut versions = Vec::new();
        let mut i = 0;
        while i + 2 < lines.len() {
            let commit_hash = lines[i].to_string();
            let message = lines[i + 1].to_string();
            let timestamp = lines[i + 2].to_string();

            versions.push(KnowledgeVersion {
                commit_hash,
                message,
                timestamp,
                agent_id: agent_id.to_string(),
            });

            i += 3;
        }

        Ok(versions)
    }

    /// Get the content of a specific version.
    pub async fn get_version(
        &self,
        agent_id: &str,
        commit_hash: &str,
    ) -> Result<String, KnowledgeGitError> {
        self.ensure_repo(agent_id).await?;

        let repo_path = self.agent_repo_path(agent_id);

        // Get the file content from a specific commit
        let output = Command::new("git")
            .args(["show", &format!("{}:knowledge.md", commit_hash)])
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            if stderr.contains("does not exist") || stderr.contains("no such path") {
                return Err(KnowledgeGitError::NotFound(format!(
                    "Commit {} not found or file doesn't exist",
                    commit_hash
                )));
            }
            return Err(KnowledgeGitError::GitCommand(format!(
                "Failed to get version: {}",
                stderr
            )));
        }

        String::from_utf8(output.stdout)
            .map_err(|_| KnowledgeGitError::InvalidUtf8)
    }

    /// Get a unified diff between two versions.
    pub async fn diff_versions(
        &self,
        agent_id: &str,
        hash_a: &str,
        hash_b: &str,
    ) -> Result<String, KnowledgeGitError> {
        self.ensure_repo(agent_id).await?;

        let repo_path = self.agent_repo_path(agent_id);

        // Get the diff between two commits
        let output = Command::new("git")
            .args(["diff", hash_a, hash_b])
            .current_dir(&repo_path)
            .output()
            .map_err(KnowledgeGitError::Io)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(KnowledgeGitError::GitCommand(format!(
                "Failed to generate diff: {}",
                stderr
            )));
        }

        String::from_utf8(output.stdout)
            .map_err(|_| KnowledgeGitError::InvalidUtf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_init_repo() {
        let temp_dir = TempDir::new().unwrap();
        let store = GitCliKnowledgeStore::new(temp_dir.path().to_path_buf());

        let result = store.ensure_repo("test-agent").await;
        assert!(result.is_ok());

        let repo_path = store.agent_repo_path("test-agent");
        assert!(repo_path.join(".git").exists());
    }

    #[tokio::test]
    async fn test_commit_version() {
        let temp_dir = TempDir::new().unwrap();
        let store = GitCliKnowledgeStore::new(temp_dir.path().to_path_buf());

        let content = "# Agent Knowledge\n\nInitial knowledge base.";
        let commit_hash = store
            .commit_version("test-agent", content, "Initial commit")
            .await;

        assert!(commit_hash.is_ok());
        let hash = commit_hash.unwrap();
        assert!(!hash.is_empty());
    }

    #[tokio::test]
    async fn test_list_versions() {
        let temp_dir = TempDir::new().unwrap();
        let store = GitCliKnowledgeStore::new(temp_dir.path().to_path_buf());

        store
            .commit_version("test-agent", "Content 1", "First commit")
            .await
            .unwrap();
        store
            .commit_version("test-agent", "Content 2", "Second commit")
            .await
            .unwrap();

        let versions = store.list_versions("test-agent", 10).await.unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].message, "Second commit");
        assert_eq!(versions[1].message, "First commit");
    }

    #[tokio::test]
    async fn test_get_version() {
        let temp_dir = TempDir::new().unwrap();
        let store = GitCliKnowledgeStore::new(temp_dir.path().to_path_buf());

        let content = "Test knowledge content";
        let hash = store
            .commit_version("test-agent", content, "Test commit")
            .await
            .unwrap();

        let retrieved = store.get_version("test-agent", &hash).await.unwrap();
        assert_eq!(retrieved, content);
    }

    #[tokio::test]
    async fn test_diff_versions() {
        let temp_dir = TempDir::new().unwrap();
        let store = GitCliKnowledgeStore::new(temp_dir.path().to_path_buf());

        let hash1 = store
            .commit_version("test-agent", "Content A", "First")
            .await
            .unwrap();
        let hash2 = store
            .commit_version("test-agent", "Content B", "Second")
            .await
            .unwrap();

        let diff = store.diff_versions("test-agent", &hash1, &hash2).await;
        assert!(diff.is_ok());

        let diff_str = diff.unwrap();
        assert!(diff_str.contains("Content A") || diff_str.contains("Content B"));
    }

    #[tokio::test]
    async fn test_get_nonexistent_version() {
        let temp_dir = TempDir::new().unwrap();
        let store = GitCliKnowledgeStore::new(temp_dir.path().to_path_buf());

        store
            .commit_version("test-agent", "Content", "Commit")
            .await
            .unwrap();

        let result = store.get_version("test-agent", "nonexistent").await;
        assert!(result.is_err());
    }
}
