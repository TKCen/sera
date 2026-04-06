#!/bin/bash
# rebuild.sh — Rebuild and restart the SERA dev stack
# Usage: ./scripts/rebuild.sh [--worker] [--shadow]
#
# Flags:
#   --worker   Also rebuild the agent-worker image
#   --shadow   Include shadow proxy overlay

set -euo pipefail

cd "$(dirname "$0")/.."

COMPOSE_FILES="-f docker-compose.yaml -f docker-compose.dev.yaml"
REBUILD_WORKER=false

for arg in "$@"; do
  case $arg in
    --worker) REBUILD_WORKER=true ;;
    --shadow) COMPOSE_FILES="$COMPOSE_FILES -f docker-compose.shadow.yaml" ;;
  esac
done

echo "=== SERA Stack Rebuild ==="
echo "Compose files: $COMPOSE_FILES"

# 1. Pull latest code (already done if you're running this)
echo ""
echo "--- Step 1: Git pull ---"
git pull --rebase 2>/dev/null || true

# 2. Rebuild agent worker image if requested
if [ "$REBUILD_WORKER" = true ]; then
  echo ""
  echo "--- Step 2: Rebuilding agent-worker image ---"
  docker build -f core/sandbox/Dockerfile.worker -t sera-agent-worker:latest core/
fi

# 3. Restart the stack (rebuilds sera-core and sera-web from dev compose)
echo ""
echo "--- Step 3: Restarting stack ---"
docker compose $COMPOSE_FILES down
docker compose $COMPOSE_FILES up -d --build

# 4. Wait for health
echo ""
echo "--- Step 4: Waiting for health ---"
for i in $(seq 1 30); do
  if curl -sf http://localhost:3001/api/health > /dev/null 2>&1; then
    echo "sera-core healthy after ${i}s"
    break
  fi
  sleep 1
done

# 5. Show status
echo ""
echo "--- Stack status ---"
docker compose $COMPOSE_FILES ps --format "table {{.Name}}\t{{.Status}}\t{{.Ports}}" 2>/dev/null || docker compose $COMPOSE_FILES ps
echo ""
echo "Done! Stack rebuilt and running."
