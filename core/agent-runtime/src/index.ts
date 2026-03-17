/**
 * Agent Runtime Entrypoint — bootstraps the autonomous reasoning actor.
 *
 * This process runs inside an agent's Docker container. It:
 *  1. Loads configuration from environment variables
 *  2. Reads the AGENT.yaml manifest from the workspace
 *  3. Initializes the LLM client, tool executor, and Centrifugo publisher
 *  4. Starts the heartbeat
 *  5. Enters the reasoning loop (processes tasks from stdin)
 *
 * Environment variables (injected by Orchestrator):
 *   SERA_IDENTITY_TOKEN  — JWT for authenticating with Core services
 *   SERA_CORE_URL        — Base URL for the Core API (e.g. http://sera-core:3001)
 *   CENTRIFUGO_API_URL   — Centrifugo HTTP API URL
 *   CENTRIFUGO_API_KEY   — Centrifugo API key
 *   AGENT_NAME           — Display name of the agent
 *   AGENT_INSTANCE_ID    — Unique instance ID
 */

import fs from 'fs';
import path from 'path';
import readline from 'readline';
import { loadManifest } from './manifest.js';
import { LLMClient } from './llmClient.js';
import { RuntimeToolExecutor } from './tools.js';
import { CentrifugoPublisher } from './centrifugo.js';
import { ReasoningLoop } from './loop.js';
import { startHeartbeat } from './heartbeat.js';
import { log } from './logger.js';

// ── Configuration ────────────────────────────────────────────────────────────

const SERA_IDENTITY_TOKEN = process.env.SERA_IDENTITY_TOKEN;
const SERA_CORE_URL = process.env.SERA_CORE_URL || 'http://sera-core:3001';
const CENTRIFUGO_API_URL = process.env.CENTRIFUGO_API_URL || 'http://centrifugo:8000/api';
const CENTRIFUGO_API_KEY = process.env.CENTRIFUGO_API_KEY || 'sera-api-key';
const AGENT_NAME = process.env.AGENT_NAME || 'unknown-agent';
const AGENT_INSTANCE_ID = process.env.AGENT_INSTANCE_ID || 'unknown-instance';
const WORKSPACE_PATH = '/workspace';

// ── Bootstrap ────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  log('info', `Starting SERA Agent Runtime v1.0.0`);
  log('info', `Agent: ${AGENT_NAME} (${AGENT_INSTANCE_ID})`);
  log('info', `Core URL: ${SERA_CORE_URL}`);

  // Validate required env vars
  if (!SERA_IDENTITY_TOKEN) {
    log('error', 'SERA_IDENTITY_TOKEN not set — cannot authenticate with Core');
    process.exit(1);
  }

  // ── Load Manifest ──────────────────────────────────────────────────────────
  const manifestPaths = [
    path.join(WORKSPACE_PATH, 'AGENT.yaml'),
    path.join(WORKSPACE_PATH, `${AGENT_NAME}.agent.yaml`),
  ];

  let manifestPath: string | undefined;
  for (const p of manifestPaths) {
    if (fs.existsSync(p)) {
      manifestPath = p;
      break;
    }
  }

  if (!manifestPath) {
    log('error', `No manifest found. Tried: ${manifestPaths.join(', ')}`);
    log('info', 'Workspace contents:');
    try {
      const files = fs.readdirSync(WORKSPACE_PATH);
      for (const f of files) {
        log('info', `  ${f}`);
      }
    } catch {
      log('error', 'Could not list workspace directory');
    }
    process.exit(1);
  }

  const manifest = loadManifest(manifestPath);
  log('info', `Loaded manifest: ${manifest.metadata.displayName} (${manifest.metadata.name})`);

  // ── Initialize Services ────────────────────────────────────────────────────
  const llmClient = new LLMClient(SERA_CORE_URL, SERA_IDENTITY_TOKEN, manifest.model.name);
  const toolExecutor = new RuntimeToolExecutor(WORKSPACE_PATH);
  const centrifugo = new CentrifugoPublisher(
    CENTRIFUGO_API_URL,
    CENTRIFUGO_API_KEY,
    manifest.metadata.name,
    manifest.metadata.displayName,
  );

  const loop = new ReasoningLoop(llmClient, toolExecutor, centrifugo, manifest);

  // ── Start Heartbeat ────────────────────────────────────────────────────────
  const stopHeartbeat = startHeartbeat(
    SERA_CORE_URL,
    AGENT_INSTANCE_ID,
    SERA_IDENTITY_TOKEN,
  );

  // ── Announce Readiness ─────────────────────────────────────────────────────
  await centrifugo.publishThought('observe', `🟢 Agent runtime started — ready for tasks`);
  log('info', 'Agent runtime ready — waiting for input on stdin');

  // ── Read Tasks from Stdin ──────────────────────────────────────────────────
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
    terminal: false,
  });

  rl.on('line', async (line) => {
    const task = line.trim();
    if (!task) return;

    log('info', `Received task: "${task.substring(0, 100)}..."`);
    await centrifugo.publishThought('observe', `📥 Received task: "${task.substring(0, 80)}..."`);

    const result = await loop.run(task);
    log('info', `Task complete: ${result.substring(0, 100)}...`);

    // Output result to stdout for capture
    console.log(`\n--- RESULT ---\n${result}\n--- END ---\n`);
  });

  rl.on('close', () => {
    log('info', 'stdin closed — shutting down');
    stopHeartbeat();
    process.exit(0);
  });

  // ── Graceful Shutdown ──────────────────────────────────────────────────────
  process.on('SIGTERM', () => {
    log('info', 'SIGTERM received — shutting down');
    stopHeartbeat();
    rl.close();
    process.exit(0);
  });

  process.on('SIGINT', () => {
    log('info', 'SIGINT received — shutting down');
    stopHeartbeat();
    rl.close();
    process.exit(0);
  });
}

// ── Run ──────────────────────────────────────────────────────────────────────

main().catch((err) => {
  log('error', `Fatal error: ${err.message || err}`);
  process.exit(1);
});
