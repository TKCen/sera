#!/bin/bash
set -euo pipefail

# SERA Discord Deployment Script
# This script sets up and runs a SERA instance with Discord connectivity

echo "🚀 Starting SERA Discord Deployment"

# Check if Discord token is configured
if ! ./rust/target/debug/sera secrets get connectors/discord-main/token >/dev/null 2>&1; then
    echo "❌ Discord token not configured. Please run:"
    echo "   ./rust/target/debug/sera secrets set connectors/discord-main/token YOUR_BOT_TOKEN"
    exit 1
fi

# Check if LLM provider is accessible (if using local)
if grep -q "localhost:1234" sera.yaml; then
    echo "🔍 Checking LM Studio connectivity..."
    if ! curl -s -f http://localhost:1234/v1/models >/dev/null 2>&1; then
        echo "⚠️  LM Studio not accessible at localhost:1234"
        echo "   Please ensure LM Studio is running and serving on port 1234"
        echo "   Or update the provider configuration in sera.yaml"
    else
        echo "✅ LM Studio is accessible"
    fi
fi

# Set environment for optimal logging
export RUST_LOG=${RUST_LOG:-"sera=info,sera_gateway=info,sera_runtime=info"}

# Set the runtime binary path so the gateway can find sera-runtime
export SERA_RUNTIME_BIN="./rust/target/debug/sera-runtime"

echo "📋 Configuration Summary:"
echo "  - Config file: sera.yaml"
echo "  - Database: sera.db (SQLite)"
echo "  - Secrets: secrets/ directory"
echo "  - Log level: $RUST_LOG"
echo "  - Runtime binary: $SERA_RUNTIME_BIN"

echo ""
echo "🎯 Starting SERA Gateway..."
echo "  - HTTP API will be available at http://localhost:3001"
echo "  - Discord connector will auto-connect"
echo "  - Use Ctrl+C to stop gracefully"
echo ""

# Start the SERA gateway
./rust/target/debug/sera start --config sera.yaml --port 3001