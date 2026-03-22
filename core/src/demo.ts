import path from 'path';
import { Orchestrator } from './agents/Orchestrator.js';

async function main() {
  console.log('[Demo] --- Starting SERA Multi-Agent Demo ---');

  const orchestrator = new Orchestrator();
  const agentsDir = path.resolve(import.meta.dirname, '..', '..', 'agents');
  orchestrator.loadTemplates(agentsDir);

  console.log('[Demo] \n--- Loaded Agents ---');
  for (const agent of orchestrator.listAgents()) {
    console.log(`[Demo]   - ${agent.name} [${agent.status}] (Started: ${agent.startTime.toISOString()})`);
  }

  console.log('[Demo] \n--- Scenario 1: Simple Task ---');
  const result1 = await orchestrator.executeTask('Hello, who are you?');
  console.log('[Demo] Result 1:', result1);

  console.log('[Demo] \n--- Scenario 2: Task with Delegation (Research) ---');
  const result2 = await orchestrator.executeTask('I need some research on Model Context Protocol.');
  console.log('[Demo] Result 2:', result2);

  console.log('[Demo] \n--- Demo Completed ---');
}

main().catch((err) => console.error('[Demo] Error:', err));
