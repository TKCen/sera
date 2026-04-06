#!/usr/bin/env node

/**
 * CLI utility to create a task for a SERA bridge agent.
 *
 * Usage:
 *   node --experimental-strip-types tools/bridges/cli/create-task.ts \
 *     --agent omc-bridge \
 *     --prompt "Refactor the auth module" \
 *     --timeout 30m
 *
 *   node --experimental-strip-types tools/bridges/cli/create-task.ts \
 *     --prompt "Fix lint errors in core/src/routes/"
 */

const SERA_CORE_URL = process.env.SERA_CORE_URL || 'http://localhost:3001';
const SERA_API_KEY = process.env.SERA_API_KEY || 'sera_bootstrap_dev_123';

interface ParsedArgs {
  agent?: string;
  prompt?: string;
  timeout?: string;
}

interface AgentInstance {
  id: string;
  name: string;
  lifecycle_mode: string;
}

interface TaskResponse {
  taskId: string;
  status: string;
  priority: number;
}

/**
 * Parse command-line arguments into key-value pairs.
 * Supports: --key value
 */
function parseArgs(): ParsedArgs {
  const args: ParsedArgs = {};
  for (let i = 2; i < process.argv.length; i++) {
    const arg = process.argv[i];
    if (arg.startsWith('--')) {
      const key = arg.slice(2);
      const value = process.argv[i + 1];
      if (value && !value.startsWith('--')) {
        args[key as keyof ParsedArgs] = value;
        i++; // skip the value in next iteration
      }
    }
  }
  return args;
}

/**
 * Resolve agent name to agent instance ID.
 * First tries exact name match, then falls back to agent lookup by name.
 */
async function resolveAgentId(agentName: string): Promise<string> {
  try {
    // Try to get agent by name from instances endpoint
    const instancesUrl = new URL('/api/agents/instances', SERA_CORE_URL);
    instancesUrl.searchParams.set('name', agentName);

    const resp = await fetch(instancesUrl.toString(), {
      method: 'GET',
      headers: {
        Authorization: `Bearer ${SERA_API_KEY}`,
      },
    });

    if (!resp.ok) {
      throw new Error(`Failed to fetch agent instances: ${resp.status} ${resp.statusText}`);
    }

    const instances: AgentInstance[] = await resp.json();
    if (instances.length === 0) {
      throw new Error(`Agent "${agentName}" not found`);
    }

    const agent = instances.find((a) => a.name === agentName);
    if (!agent) {
      throw new Error(`Agent "${agentName}" not found among instances`);
    }

    return agent.id;
  } catch (err) {
    throw new Error(`Failed to resolve agent "${agentName}": ${(err as Error).message}`);
  }
}

/**
 * Create a task for the given agent.
 */
async function createTask(
  agentId: string,
  prompt: string,
  timeout?: string
): Promise<TaskResponse> {
  // Parse timeout if provided (e.g., "30m" → context.timeout)
  const context = timeout ? { timeout } : undefined;

  const tasksUrl = new URL(`/api/agents/${agentId}/tasks`, SERA_CORE_URL);

  const resp = await fetch(tasksUrl.toString(), {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      Authorization: `Bearer ${SERA_API_KEY}`,
    },
    body: JSON.stringify({
      task: prompt,
      context,
      priority: 100,
    }),
  });

  if (!resp.ok) {
    const errorBody = await resp.text();
    throw new Error(
      `Failed to create task: ${resp.status} ${resp.statusText}\n${errorBody}`
    );
  }

  const result: TaskResponse = await resp.json();
  return result;
}

/**
 * Main entry point.
 */
async function main(): Promise<void> {
  try {
    const args = parseArgs();

    // Validate required arguments
    if (!args.prompt) {
      console.error('Error: --prompt is required');
      console.error(
        '\nUsage: node --experimental-strip-types tools/bridges/cli/create-task.ts'
      );
      console.error('  --prompt <text>           [required] Task prompt');
      console.error('  --agent <name>            [optional] Agent name (default: omc-bridge)');
      console.error('  --timeout <duration>      [optional] Timeout (e.g., 30m)');
      process.exit(1);
    }

    // Resolve agent ID
    const agentName = args.agent || 'omc-bridge';
    console.log(`Resolving agent: ${agentName}`);
    const agentId = await resolveAgentId(agentName);
    console.log(`  → ID: ${agentId}`);

    // Create task
    console.log(`Creating task...`);
    const task = await createTask(agentId, args.prompt, args.timeout);
    console.log(`Task created: ${task.taskId}`);
    console.log(`Status: ${task.status}`);
    console.log(`Priority: ${task.priority}`);
  } catch (err) {
    console.error('Error:', (err as Error).message);
    process.exit(1);
  }
}

main();
