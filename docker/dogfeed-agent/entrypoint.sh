#!/usr/bin/env bash
set -euo pipefail

# SERA Dogfeed Agent Entrypoint
# Env vars:
#   DOGFEED_TASK       - The task description to work on
#   DOGFEED_BRANCH     - The branch name to work on
#   DOGFEED_AGENT      - Agent type: "omc" or "pi-agent"
#   DOGFEED_REPO_PATH  - Path to the mounted repo (default: /workspace)

REPO="${DOGFEED_REPO_PATH:-/workspace}"
BRANCH="${DOGFEED_BRANCH:-dogfeed/unnamed}"
TASK="${DOGFEED_TASK:-}"
AGENT="${DOGFEED_AGENT:-omc}"

if [ -z "$TASK" ]; then
  echo "ERROR: DOGFEED_TASK not set"
  exit 1
fi

cd "$REPO"

# Configure git
git config user.name "SERA Dogfeed"
git config user.email "dogfeed@sera.dev"

# Create and switch to the dogfeed branch
git checkout -b "$BRANCH" main 2>/dev/null || git checkout "$BRANCH"

echo "=== SERA Dogfeed Agent ==="
echo "Task:   $TASK"
echo "Branch: $BRANCH"
echo "Agent:  $AGENT"
echo "========================="

# Build the prompt for the coding agent
PROMPT="You are working on the SERA codebase. Your task:

${TASK}

Rules:
- Make the MINIMAL change that satisfies the task
- Follow existing code patterns and conventions
- Do not modify unrelated files
- Do not add unnecessary comments or documentation
- Run tests after making changes

After completing the task, commit your changes with message:
dogfeed: ${TASK}"

if [ "$AGENT" = "omc" ]; then
  # Run Claude Code (OMC) in non-interactive mode
  if command -v claude &>/dev/null; then
    echo "$PROMPT" | claude --print --dangerously-skip-permissions
  else
    echo "ERROR: claude CLI not found"
    exit 1
  fi
elif [ "$AGENT" = "pi-agent" ]; then
  # Run pi-agent with local Qwen model
  if command -v pi &>/dev/null; then
    pi --model "${PI_AGENT_MODEL:-qwen/qwen3.5-35b-a3b}" \
       --provider "${PI_AGENT_PROVIDER:-lmstudio}" \
       --print --no-session "$PROMPT"
  else
    echo "ERROR: pi CLI not found"
    exit 1
  fi
else
  echo "ERROR: Unknown agent type: $AGENT"
  exit 1
fi

echo "=== Agent execution complete ==="
