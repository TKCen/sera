---
id: git-workflow
name: Git Workflow
version: 1.0.0
description: Standard git workflow for agentic development.
triggers: ["git", "branch", "commit", "pr", "pull-request"]
category: engineering/workflow
tags: ["git", "workflow", "vcs"]
---

# Git Workflow

## Branching
- All work should happen on a feature branch (`feat/`), bugfix branch (`fix/`), or task branch (`task/`).
- Branch names should be kebab-case and include a brief description.

## Commits
- Use descriptive commit messages following Conventional Commits (e.g. `feat: add skill loader`).
- Keep commits atomic and focused.

## Merging
- Always pull the latest changes from the base branch before finalizing work.
- Use `git worktree` when working on multiple tasks concurrently to ensure isolation.
