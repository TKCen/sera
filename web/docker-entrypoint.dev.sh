#!/bin/sh
set -e

# Install dependencies into the named volume if missing or stale
if [ ! -d node_modules ] || [ package.json -nt node_modules/.package-lock.json ]; then
  echo "Installing dependencies..."
  npm install
fi

exec "$@"
