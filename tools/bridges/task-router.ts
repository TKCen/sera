/**
 * Task Router — selects the best bridge agent for a task.
 *
 * Routing rules (in priority order):
 *   1. If preferredTool is set and not 'auto', route directly to that bridge.
 *   2. complexity 'trivial' → omo-bridge (cheapest)
 *   3. complexity 'complex' → omc-bridge (best reasoning)
 *   4. domain 'google' or prompt mentions Google/GCP/Firebase → gemini-bridge
 *   5. Default (medium or unset) → omc-bridge
 *
 * Queue-depth tie-breaking: when two bridges both match, prefer the one with
 * fewer queued+running tasks (via GET /api/agents from sera API).
 *
 * Spec: docs/BRIDGE-AGENT-SPEC.md
 */

// ── Config ────────────────────────────────────────────────────────────────────

const SERA_CORE_URL = process.env['SERA_CORE_URL'] ?? 'http://localhost:3001';
const SERA_API_KEY = process.env['SERA_API_KEY'] ?? 'sera_bootstrap_dev_123';

// ── Types ─────────────────────────────────────────────────────────────────────

export interface TaskMetadata {
  prompt: string;
  complexity?: 'trivial' | 'medium' | 'complex';
  domain?: string;
  preferredTool?: 'omc' | 'omo' | 'omx' | 'gemini' | 'auto';
}

export interface RoutingResult {
  /** e.g. 'omc-bridge' */
  agentName: string;
  /** UUID from sera API */
  agentId: string;
  /** Human-readable rationale */
  reason: string;
}

/** Subset of the agent instance shape returned by GET /api/agents */
interface AgentInstance {
  id: string;
  name: string;
  status: string;
  lifecycle_mode: string;
}

// ── Logger ────────────────────────────────────────────────────────────────────

function log(
  level: 'info' | 'warn' | 'error' | 'debug',
  msg: string,
  extra?: Record<string, unknown>
): void {
  const entry: Record<string, unknown> = {
    ts: new Date().toISOString(),
    level,
    msg,
    ...extra,
  };
  // eslint-disable-next-line no-console
  console.error(JSON.stringify(entry));
}

// ── HTTP helper ───────────────────────────────────────────────────────────────

async function seraFetch(path: string, init?: RequestInit): Promise<Response> {
  const url = `${SERA_CORE_URL}${path}`;
  const headers: Record<string, string> = {
    Authorization: `Bearer ${SERA_API_KEY}`,
    ...(init?.headers as Record<string, string> | undefined),
  };
  return globalThis.fetch(url, { ...init, headers });
}

// ── Bridge agent discovery ────────────────────────────────────────────────────

/** Fetch all registered bridge agent instances from the sera API. */
async function fetchBridgeAgents(): Promise<AgentInstance[]> {
  const res = await seraFetch('/api/agents/instances');
  if (!res.ok) {
    throw new Error(`Failed to fetch agent instances: HTTP ${res.status}`);
  }
  const data = (await res.json()) as AgentInstance[];
  // Filter to only bridge instances (persistent, name ends with '-bridge')
  return data.filter(
    (inst) => inst.lifecycle_mode === 'persistent' && inst.name.endsWith('-bridge')
  );
}

/** Fetch queue depth (queued + running tasks) for a given agent UUID via the task stats endpoint. */
async function fetchQueueDepth(agentId: string): Promise<number> {
  const res = await seraFetch(`/api/agents/${agentId}/tasks?status=queued,running&limit=0`);
  if (!res.ok) {
    // Non-fatal — treat as unknown depth (0) so routing still proceeds
    log('warn', 'Could not fetch queue depth', { agentId, status: res.status });
    return 0;
  }
  // The tasks endpoint may return a body with a `total` count, or an array.
  const data = (await res.json()) as { total?: number } | unknown[];
  if (Array.isArray(data)) return data.length;
  if (typeof data === 'object' && data !== null && typeof (data as { total?: number }).total === 'number') {
    return (data as { total: number }).total;
  }
  return 0;
}

// ── Routing logic ─────────────────────────────────────────────────────────────

/** Map a tool preference/rule to its bridge name. */
const TOOL_TO_BRIDGE: Record<string, string> = {
  omc: 'omc-bridge',
  omo: 'omo-bridge',
  omx: 'omx-bridge',
  gemini: 'gemini-bridge',
};

/** Returns true if the prompt or domain suggests a Google-ecosystem task. */
function isGoogleDomain(metadata: TaskMetadata): boolean {
  if (metadata.domain?.toLowerCase() === 'google') return true;
  const lower = metadata.prompt.toLowerCase();
  return (
    lower.includes('google') ||
    lower.includes('gcp') ||
    lower.includes('firebase') ||
    lower.includes('bigquery') ||
    lower.includes('cloud run') ||
    lower.includes('google cloud')
  );
}

/**
 * Determine the candidate bridge name from routing rules (before queue-depth
 * tie-breaking). Returns the bridge name, e.g. 'omc-bridge', and the reason.
 */
function selectCandidateBridge(metadata: TaskMetadata): { bridgeName: string; reason: string } {
  // Rule 1 — explicit tool preference
  if (metadata.preferredTool && metadata.preferredTool !== 'auto') {
    const bridgeName = TOOL_TO_BRIDGE[metadata.preferredTool];
    if (!bridgeName) {
      throw new Error(`Unknown preferredTool: ${metadata.preferredTool}`);
    }
    return {
      bridgeName,
      reason: `Explicit preferredTool=${metadata.preferredTool}`,
    };
  }

  // Rule 2 — trivial complexity → OMO (cheapest)
  if (metadata.complexity === 'trivial') {
    return { bridgeName: 'omo-bridge', reason: 'complexity=trivial routed to OMO (cheapest)' };
  }

  // Rule 3 — complex → OMC (best reasoning)
  if (metadata.complexity === 'complex') {
    return { bridgeName: 'omc-bridge', reason: 'complexity=complex routed to OMC (best reasoning)' };
  }

  // Rule 4 — Google/GCP/Firebase domain or keywords → Gemini
  if (isGoogleDomain(metadata)) {
    return { bridgeName: 'gemini-bridge', reason: 'Google/GCP/Firebase domain detected, routed to Gemini' };
  }

  // Rule 5 — default (medium or unset) → OMC
  return { bridgeName: 'omc-bridge', reason: 'Default routing to OMC (medium/unset complexity)' };
}

// ── Main export ───────────────────────────────────────────────────────────────

/**
 * Select the best bridge agent for a task.
 *
 * Fetches live agent instances from sera, applies routing rules, and
 * resolves ties using queue depth.
 */
export async function routeTask(metadata: TaskMetadata): Promise<RoutingResult> {
  log('debug', 'routeTask called', {
    complexity: metadata.complexity,
    domain: metadata.domain,
    preferredTool: metadata.preferredTool,
    promptSnippet: metadata.prompt.slice(0, 80),
  });

  // Determine primary candidate
  const { bridgeName: primaryBridge, reason: primaryReason } = selectCandidateBridge(metadata);

  // Fetch live bridge agents
  let agents: AgentInstance[];
  try {
    agents = await fetchBridgeAgents();
  } catch (err) {
    throw new Error(`Task routing failed — could not list agents: ${String(err)}`);
  }

  log('debug', 'Discovered bridge agents', {
    count: agents.length,
    names: agents.map((a) => a.name),
  });

  // Find the primary candidate in the live agent list
  const primaryAgent = agents.find((a) => a.name === primaryBridge);

  if (!primaryAgent) {
    throw new Error(
      `Routing selected bridge "${primaryBridge}" but it is not registered with sera. ` +
        `Registered bridges: ${agents.map((a) => a.name).join(', ') || 'none'}`
    );
  }

  // For explicit tool routing (no tie-breaking needed), return immediately
  if (metadata.preferredTool && metadata.preferredTool !== 'auto') {
    log('info', 'Routing decision (explicit preference)', {
      agentName: primaryAgent.name,
      agentId: primaryAgent.id,
      reason: primaryReason,
    });
    return { agentName: primaryAgent.name, agentId: primaryAgent.id, reason: primaryReason };
  }

  // Queue-depth tie-breaking: find any other bridge that also satisfies the rule,
  // and prefer the one with fewer queued tasks.
  //
  // For trivial → OMO and Google → Gemini there is exactly one match, so tie-breaking
  // only matters for the OMC default case where both omc-bridge might be present
  // alongside other alternatives. We compare the primary candidate against any same-
  // name duplicates (shouldn't happen) or — for the default OMC rule — also consider
  // omo-bridge as a fallback if it has a substantially lower queue.
  //
  // Practical tie-breaking: resolve depth for primary, and compare against any other
  // registered bridge of the same logical type.

  const primaryDepth = await fetchQueueDepth(primaryAgent.id);

  log('debug', 'Queue depth for primary candidate', {
    agentName: primaryAgent.name,
    agentId: primaryAgent.id,
    queueDepth: primaryDepth,
  });

  // Find other bridges that share the same name (shouldn't happen in practice but
  // guard against duplicate registrations).
  const sameNameAgents = agents.filter(
    (a) => a.name === primaryBridge && a.id !== primaryAgent.id
  );

  if (sameNameAgents.length === 0) {
    // No tie to break
    log('info', 'Routing decision', {
      agentName: primaryAgent.name,
      agentId: primaryAgent.id,
      queueDepth: primaryDepth,
      reason: primaryReason,
    });
    return { agentName: primaryAgent.name, agentId: primaryAgent.id, reason: primaryReason };
  }

  // Compare depths across all same-name candidates
  const allCandidates = [primaryAgent, ...sameNameAgents];
  const depths = await Promise.all(allCandidates.map((a) => fetchQueueDepth(a.id)));

  let bestIdx = 0;
  let bestDepth = depths[0] ?? 0;
  for (let i = 1; i < allCandidates.length; i++) {
    const d = depths[i] ?? 0;
    if (d < bestDepth) {
      bestDepth = d;
      bestIdx = i;
    }
  }

  const winner = allCandidates[bestIdx]!;
  const reason =
    bestIdx === 0
      ? `${primaryReason} (lowest queue depth: ${bestDepth})`
      : `${primaryReason} — tie-broken by queue depth, selected duplicate instance ${winner.id} (depth ${bestDepth})`;

  log('info', 'Routing decision (tie-broken)', {
    agentName: winner.name,
    agentId: winner.id,
    queueDepth: bestDepth,
    reason,
  });

  return { agentName: winner.name, agentId: winner.id, reason };
}
