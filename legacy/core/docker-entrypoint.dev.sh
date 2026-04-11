#!/bin/sh
set -e

# Install dependencies into the named volume if missing or stale
if [ ! -f node_modules/.bun-installed ] || [ package.json -nt node_modules/.bun-installed ]; then
  echo "Installing dependencies..."
  bun install
  touch node_modules/.bun-installed
fi

exec "$@"
