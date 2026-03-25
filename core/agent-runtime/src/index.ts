/**
 * Agent Runtime Entrypoint — bootstraps the autonomous reasoning actor.
 *
 * This process runs inside an agent's Docker container. It:
 *  1. Loads configuration from environment variables
 *  2. Reads the AGENT.yaml manifest from the workspace
 *  3. Initializes the LLM client, tool executor, and Centrifugo publisher
 *  4. Starts the heartbeat
 *  5. Reads task JSON from stdin: { taskId, task, context?, history? }
 *  6. Runs the reasoning loop, writes result.json, outputs result JSON to stdout
 *  7. For persistent agents: polls /api/agents/:id/tasks/next for follow-up tasks
 *
 * Environment variables (injected by SandboxManager at spawn):
 *   SERA_IDENTITY_TOKEN    — JWT for authenticating with Core services
 *   SERA_CORE_URL          — Base URL for the Core API (e.g. http://sera-core:3001)
 *   CENTRIFUGO_API_URL     — Centrifugo HTTP API URL
 *   CENTRIFUGO_API_KEY     — Centrifugo API key
 *   AGENT_NAME             — Template/manifest name of the agent
 *   AGENT_INSTANCE_ID      — Unique instance UUID
 *   AGENT_LIFECYCLE_MODE   — 'persistent' | 'ephemeral' (default: ephemeral)
 *   AGENT_TIER             — Tier number (1|2|3, default: 2)
 *   TASK_POLL_INTERVAL_MS  — How often to poll for next task (default: 2000)
 *   LLM_TIMEOUT_MS         — LLM call timeout in ms (default: 120000)
 */

import fs from 'fs';
import path from 'path';
import readline from 'readline';
import axios from 'axios';
import { loadManifest } from './manifest.js';
import { LLMClient } from './llmClient.js';
import { RuntimeToolExecutor } from './tools.js';
import { CentrifugoPublisher, CentrifugoSubscriber } from './centrifugo.js';
import { ReasoningLoop } from './loop.js';
import type { TaskInput, TaskOutput } from './loop.js';
import { startHeartbeat } from './heartbeat.js';
import { startChatServer } from './chatServer.js';
import { log } from './logger.js';

// ── Configuration ──────────────────────────────────────────────────────────

const SERA_IDENTITY_TOKEN = process.env['SERA_IDENTITY_TOKEN'];
const SERA_CORE_URL = process.env['SERA_CORE_URL'] || 'http://sera-core:3001';
const CENTRIFUGO_API_URL = process.env['CENTRIFUGO_API_URL'] || 'http://centrifugo:8000/api';
const CENTRIFUGO_API_KEY = process.env['CENTRIFUGO_API_KEY'] || 'sera-api-key';
const AGENT_NAME = process.env['AGENT_NAME'] || 'unknown-agent';
const AGENT_INSTANCE_ID = process.env['AGENT_INSTANCE_ID'] || 'unknown-instance';
const LIFECYCLE_MODE = process.env['AGENT_LIFECYCLE_MODE'] || 'ephemeral';
const TIER = process.env['AGENT_TIER'] ? parseInt(process.env['AGENT_TIER'], 10) : 2;
const TASK_POLL_INTERVAL_MS = process.env['TASK_POLL_INTERVAL_MS']
  ? parseInt(process.env['TASK_POLL_INTERVAL_MS'], 10)
  : 2_000;
const WORKSPACE_PATH = '/workspace';
const RESULT_DIR = path.join(WORKSPACE_PATH, '.sera');
const RESULT_FILE = path.join(RESULT_DIR, 'result.json');

// ── Bootstrap ──────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  log('info', `Starting SERA Agent Runtime`);
  log('info', `Agent: ${AGENT_NAME} (${AGENT_INSTANCE_ID}) lifecycle=${LIFECYCLE_MODE} tier=${TIER}`);
  log('info', `Core URL: ${SERA_CORE_URL}`);

  if (!SERA_IDENTITY_TOKEN) {
    log('error', 'SERA_IDENTITY_TOKEN not set — cannot authenticate with Core');
    process.exit(1);
  }

  // ── Load Manifest ────────────────────────────────────────────────────────
  const manifestPaths = [
    path.join(WORKSPACE_PATH, 'AGENT.yaml'),
    path.join(WORKSPACE_PATH, `${AGENT_NAME}.agent.yaml`),
    path.join('/agents', `${AGENT_NAME}.agent.yaml`),
  ];

  let manifestPath: string | undefined;
  for (const p of manifestPaths) {
    if (fs.existsSync(p)) { manifestPath = p; break; }
  }

  if (!manifestPath) {
    log('error', `No manifest found. Tried: ${manifestPaths.join(', ')}`);
    process.exit(1);
  }

  const manifest = loadManifest(manifestPath);
  log('info', `Loaded manifest: ${manifest.metadata.displayName} (${manifest.metadata.name})`);

  // ── Initialize Services ──────────────────────────────────────────────────
  const llmClient = new LLMClient(SERA_CORE_URL, SERA_IDENTITY_TOKEN, manifest.model.name);
  const toolExecutor = new RuntimeToolExecutor(WORKSPACE_PATH, TIER);

  // CentrifugoPublisher uses AGENT_INSTANCE_ID as the agentId for channel names
  const centrifugo = new CentrifugoPublisher(
    CENTRIFUGO_API_URL,
    CENTRIFUGO_API_KEY,
    AGENT_INSTANCE_ID,
    manifest.metadata.name,
  );

  const loop = new ReasoningLoop(llmClient, toolExecutor, centrifugo, manifest);

  // ── Initialize Subscriber (Story 9.3) ────────────────────────────────────
  const CENTRIFUGO_WS_URL = CENTRIFUGO_API_URL.replace('/api', '/connection/websocket').replace('http', 'ws');
  
  try {
    const tokenRes = await axios.get<{ token: string }>(`${SERA_CORE_URL}/api/intercom/centrifugo/token?agentId=${AGENT_INSTANCE_ID}`, {
      headers: { Authorization: `Bearer ${SERA_IDENTITY_TOKEN}` }
    });
    
    const subscriber = new CentrifugoSubscriber(CENTRIFUGO_WS_URL, tokenRes.data.token, AGENT_INSTANCE_ID);
    await subscriber.connect();

    // Subscribe to all permitted peer private channels
    const channels = (manifest.intercom?.canMessage ?? []).map(peerId => {
      // Sort IDs for canonical channel name
      const pair = [manifest.metadata.name, peerId].sort();
      return `private:${pair[0]}:${pair[1]}`;
    });

    for (const channel of channels) {
      await subscriber.subscribe(channel, (msg) => {
        if (msg.source.agent !== manifest.metadata.name) {
          loop.receiveIncomingMessage(msg.source.agent, JSON.stringify(msg.payload));
        }
      });
    }

    // Subscribe to circle channels (Story 9.4)
    const agentCircle = manifest.metadata.circle;
    const additionalCircles = manifest.metadata.additionalCircles ?? [];
    const circleIds = [agentCircle, ...additionalCircles].filter((c): c is string => !!c);

    for (const cId of circleIds) {
      const channel = `circle:${cId}`;
      await subscriber.subscribe(channel, (msg) => {
        // Don't inject our own broadcasts into the loop
        if (msg.source.agent !== manifest.metadata.name) {
          loop.receiveIncomingMessage(msg.source.agent, JSON.stringify(msg.payload), `circle:${cId}`);
        }
      });
    }
  } catch (err) {
    log('warn', `Failed to initialize Centrifugo subscriber: ${err instanceof Error ? err.message : String(err)}`);
  }

  // ── Start Heartbeat ──────────────────────────────────────────────────────
  const stopHeartbeat = startHeartbeat(SERA_CORE_URL, AGENT_INSTANCE_ID, SERA_IDENTITY_TOKEN);

  // ── Start Chat Server ───────────────────────────────────────────────────
  const chatBusy = false;
  const chatServer = startChatServer(loop, () => chatBusy);

  // ── Graceful Shutdown Setup ──────────────────────────────────────────────
  let currentTaskId: string | null = null;

  const shutdown = async (signal: string) => {
    log('info', `${signal} received — beginning graceful shutdown`);
    loop.shutdownRequested = true;
    chatServer.stop();

    // Give the current reasoning step up to 25s to finish
    const shutdownDeadline = Date.now() + 25_000;
    while (currentTaskId !== null && Date.now() < shutdownDeadline) {
      await sleep(200);
    }

    await centrifugo.publishThought('reflect', 'Agent runtime shutting down', 0, { anomaly: false });

    if (currentTaskId !== null) {
      const partial: Partial<TaskOutput> & { taskId: string; completedAt: string } = {
        taskId: currentTaskId,
        result: null,
        error: 'shutdown',
        exitReason: 'shutdown',
        completedAt: new Date().toISOString(),
        usage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
        thoughtStream: [],
      };
      writeResultFile(partial);
    }

    stopHeartbeat();
    process.exit(0);
  };

  process.on('SIGTERM', () => { shutdown('SIGTERM').catch(() => process.exit(1)); });
  process.on('SIGINT', () => { shutdown('SIGINT').catch(() => process.exit(1)); });

  // ── Announce Readiness ───────────────────────────────────────────────────
  await centrifugo.publishThought('observe', 'Agent runtime started — ready for tasks', 0);
  log('info', `Agent runtime ready — chat server on port ${chatServer.port}, waiting for task on stdin`);

  // ── Read Initial Task from Stdin ─────────────────────────────────────────
  const firstTask = await readTaskFromStdin();

  if (firstTask) {
    currentTaskId = firstTask.taskId;
    const output = await loop.run(firstTask);
    currentTaskId = null;

    writeResultFile({ ...output, completedAt: new Date().toISOString() });
    outputResult(output);

    // Persistent agents poll for more tasks after the initial one
    if (LIFECYCLE_MODE === 'persistent') {
      await pollTaskQueue(loop, centrifugo, (id) => { currentTaskId = id; });
    }
  } else {
    // No initial task — persistent agents go straight to polling
    if (LIFECYCLE_MODE === 'persistent') {
      await pollTaskQueue(loop, centrifugo, (id) => { currentTaskId = id; });
    } else {
      log('info', 'No task received on stdin — ephemeral agent exiting');
    }
  }

  stopHeartbeat();
  process.exit(0);
}

// ── Task Queue Polling (Story 5.8) ─────────────────────────────────────────

async function pollTaskQueue(
  loop: ReasoningLoop,
  centrifugo: CentrifugoPublisher,
  setCurrentTask: (id: string | null) => void,
): Promise<void> {
  log('info', `Persistent agent — polling for tasks every ${TASK_POLL_INTERVAL_MS}ms`);

  while (!loop.shutdownRequested) {
    try {
      const next = await fetchNextTask();

      if (next) {
        log('info', `Picked up queued task ${next.taskId}`);
        setCurrentTask(next.taskId);
        const output = await loop.run(next);
        setCurrentTask(null);

        writeResultFile({ ...output, completedAt: new Date().toISOString() });
        await submitTaskResult(next.taskId, output);
        outputResult(output);
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      log('warn', `Task poll/run error: ${msg}`);
    }

    await sleep(TASK_POLL_INTERVAL_MS);
  }
}

async function fetchNextTask(): Promise<TaskInput | null> {
  if (!SERA_IDENTITY_TOKEN) return null;
  try {
    const res = await axios.get<{ taskId: string; task: string; context?: string }>(
      `${SERA_CORE_URL}/api/agents/${AGENT_INSTANCE_ID}/tasks/next`,
      {
        headers: { Authorization: `Bearer ${SERA_IDENTITY_TOKEN}` },
        timeout: 5_000,
      },
    );
    if (res.status === 204 || !res.data) return null;
    return { taskId: res.data.taskId, task: res.data.task, context: res.data.context };
  } catch (err: unknown) {
    if (axios.isAxiosError(err) && (err.response?.status === 204 || err.response?.status === 404)) {
      return null;
    }
    throw err;
  }
}

async function submitTaskResult(taskId: string, output: TaskOutput): Promise<void> {
  if (!SERA_IDENTITY_TOKEN) return;
  try {
    await axios.post(
      `${SERA_CORE_URL}/api/agents/${AGENT_INSTANCE_ID}/tasks/${taskId}/complete`,
      output,
      {
        headers: { Authorization: `Bearer ${SERA_IDENTITY_TOKEN}`, 'Content-Type': 'application/json' },
        timeout: 10_000,
      },
    );
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    log('warn', `Failed to submit task result for ${taskId}: ${msg}`);
  }
}

// ── Helpers ────────────────────────────────────────────────────────────────

async function readTaskFromStdin(): Promise<TaskInput | null> {
  return new Promise((resolve) => {
    // If stdin is a TTY or already closed, resolve null immediately
    if (process.stdin.isTTY) { resolve(null); return; }

    const rl = readline.createInterface({ input: process.stdin, terminal: false });
    let resolved = false;

    rl.once('line', (line) => {
      rl.close();
      resolved = true;
      const trimmed = line.trim();
      if (!trimmed) { resolve(null); return; }

      try {
        const parsed = JSON.parse(trimmed) as TaskInput;
        if (!parsed.taskId || !parsed.task) {
          // Fallback: treat the whole line as the task text (backward compat)
          resolve({ taskId: `inline-${Date.now()}`, task: trimmed });
        } else {
          resolve(parsed);
        }
      } catch {
        // Non-JSON line: treat as task text
        resolve({ taskId: `inline-${Date.now()}`, task: trimmed });
      }
    });

    rl.on('close', () => {
      if (!resolved) resolve(null);
    });

    // 30s timeout — if nothing arrives, proceed without initial task
    setTimeout(() => {
      if (!resolved) {
        resolved = true;
        rl.close();
        resolve(null);
      }
    }, 30_000);
  });
}

function writeResultFile(result: Partial<TaskOutput> & { completedAt: string }): void {
  try {
    fs.mkdirSync(RESULT_DIR, { recursive: true });
    fs.writeFileSync(RESULT_FILE, JSON.stringify(result, null, 2), 'utf-8');
    log('debug', `Result written to ${RESULT_FILE}`);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    log('warn', `Failed to write result file: ${msg}`);
  }
}

function outputResult(output: TaskOutput): void {
  // Write structured JSON result to stdout for capture by sera-core
  process.stdout.write(JSON.stringify(output) + '\n');
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// ── Run ────────────────────────────────────────────────────────────────────

main().catch((err) => {
  log('error', `Fatal error: ${err instanceof Error ? err.message : err}`);
  process.exit(1);
});
