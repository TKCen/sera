#!/usr/bin/env node
/**
 * Bridge process manager — starts/stops/monitors all bridge processes via tmux.
 *
 * Usage:
 *   node --experimental-strip-types tools/bridges/manager.ts start
 *   node --experimental-strip-types tools/bridges/manager.ts stop
 *   node --experimental-strip-types tools/bridges/manager.ts status
 */

import { execSync } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const TMUX_SESSION = "sera-bridges";
const BRIDGES_DIR = path.dirname(fileURLToPath(import.meta.url));

const SERA_CORE_URL = process.env.SERA_CORE_URL ?? "http://localhost:3001";
const SERA_API_KEY = process.env.SERA_API_KEY ?? "sera_bootstrap_dev_123";

interface BridgeConfig {
  name: string;
  dir: string;
  agentName: string;
}

const BRIDGES: BridgeConfig[] = [
  { name: "omc",    dir: "omc-bridge",    agentName: "omc-bridge" },
  { name: "omo",    dir: "omo-bridge",    agentName: "omo-bridge" },
  { name: "omx",    dir: "omx-bridge",    agentName: "omx-bridge" },
  { name: "gemini", dir: "gemini-bridge",  agentName: "gemini-bridge" },
];

function tmux(args: string): string {
  try {
    return execSync(`tmux ${args}`, { encoding: "utf8" }).trim();
  } catch {
    return "";
  }
}

function sessionExists(): boolean {
  const out = tmux(`ls -F "#{session_name}" 2>/dev/null`);
  return out.split("\n").includes(TMUX_SESSION);
}

function start(): void {
  const availableBridges = BRIDGES.filter((b) =>
    existsSync(path.join(BRIDGES_DIR, b.dir, "index.ts"))
  );

  if (availableBridges.length === 0) {
    console.error("No bridge directories with index.ts found.");
    process.exit(1);
  }

  if (sessionExists()) {
    console.log(`Session '${TMUX_SESSION}' already exists — attaching.`);
    execSync(`tmux attach-session -t ${TMUX_SESSION}`, { stdio: "inherit" });
    return;
  }

  const [first, ...rest] = availableBridges;

  // Create session with first bridge in window 0
  execSync(
    `tmux new-session -d -s ${TMUX_SESSION} -c "${BRIDGES_DIR}" -x 220 -y 50`,
    { stdio: "inherit" }
  );
  execSync(
    `tmux send-keys -t ${TMUX_SESSION}:0 "node --experimental-strip-types ${first.dir}/index.ts" Enter`,
    { stdio: "inherit" }
  );
  execSync(`tmux rename-window -t ${TMUX_SESSION}:0 "${first.name}"`, { stdio: "inherit" });

  // Split a pane for each remaining bridge
  for (const bridge of rest) {
    execSync(
      `tmux split-window -t ${TMUX_SESSION} -c "${BRIDGES_DIR}"`,
      { stdio: "inherit" }
    );
    execSync(
      `tmux send-keys -t ${TMUX_SESSION} "node --experimental-strip-types ${bridge.dir}/index.ts" Enter`,
      { stdio: "inherit" }
    );
    // Even-vertical layout keeps panes readable
    execSync(`tmux select-layout -t ${TMUX_SESSION} even-vertical`, { stdio: "inherit" });
  }

  console.log(
    `Started ${availableBridges.length} bridge(s) in tmux session '${TMUX_SESSION}'.`
  );
  console.log(`Attach with: tmux attach-session -t ${TMUX_SESSION}`);
}

function stop(): void {
  if (!sessionExists()) {
    console.log(`Session '${TMUX_SESSION}' is not running.`);
    return;
  }

  // Send C-c to every pane
  const paneList = tmux(`list-panes -t ${TMUX_SESSION} -F "#{pane_id}"`);
  const panes = paneList.split("\n").filter(Boolean);

  console.log(`Sending SIGTERM to ${panes.length} pane(s)...`);
  for (const pane of panes) {
    tmux(`send-keys -t ${pane} C-c`);
  }

  // Wait up to 10 s for panes to exit
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    const running = tmux(`list-panes -t ${TMUX_SESSION} -F "#{pane_pid}" 2>/dev/null`);
    if (!running.trim()) break;
    const now = Date.now();
    while (Date.now() - now < 500) { /* spin */ }
  }

  tmux(`kill-session -t ${TMUX_SESSION}`);
  console.log(`Session '${TMUX_SESSION}' killed.`);
}

interface AgentRecord {
  name: string;
  status: string;
  queueDepth?: number;
}

async function fetchAgents(): Promise<AgentRecord[]> {
  try {
    const res = await fetch(`${SERA_CORE_URL}/api/agents`, {
      headers: { Authorization: `Bearer ${SERA_API_KEY}` },
      signal: AbortSignal.timeout(5_000),
    });
    if (!res.ok) return [];
    const data = (await res.json()) as { agents?: AgentRecord[]; data?: AgentRecord[] };
    return data.agents ?? data.data ?? [];
  } catch {
    return [];
  }
}

async function status(): Promise<void> {
  const agents = await fetchAgents();
  const sessionRunning = sessionExists();

  console.log(`\nSERA Bridge Status  [session: ${TMUX_SESSION}]`);
  console.log("-".repeat(60));
  console.log(
    `${"Bridge".padEnd(12)} ${"tmux".padEnd(8)} ${"Agent Status".padEnd(16)} Queue`
  );
  console.log("-".repeat(60));

  for (const bridge of BRIDGES) {
    const tmuxCol = sessionRunning ? "running" : "stopped";
    const agent = agents.find((a) => a.name === bridge.agentName);
    const agentCol = agent ? agent.status : "unknown";
    const queueCol =
      agent?.queueDepth !== undefined ? String(agent.queueDepth) : "-";

    console.log(
      `${bridge.name.padEnd(12)} ${tmuxCol.padEnd(8)} ${agentCol.padEnd(16)} ${queueCol}`
    );
  }

  console.log("-".repeat(60));
  if (!sessionRunning) {
    console.log(`\nRun 'manager.ts start' to launch bridges.`);
  }
}

// --- entrypoint ---

const [, , command] = process.argv;

switch (command) {
  case "start":
    start();
    break;
  case "stop":
    stop();
    break;
  case "status":
    await status();
    break;
  default:
    console.error(`Usage: manager.ts <start|stop|status>`);
    process.exit(1);
}
