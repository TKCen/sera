import path from 'path';
import { Orchestrator } from './agents/Orchestrator.js';

async function main() {
  console.log('--- Starting SERA Multi-Agent Demo ---');

  const orchestrator = new Orchestrator();
  const agentsDir = path.resolve(import.meta.dirname, '..', '..', 'agents');
  orchestrator.loadTemplates(agentsDir);

  console.log('\n--- Loaded Agents ---');
  for (const agent of orchestrator.listAgents()) {
    console.log(`  - ${agent.name} [${agent.status}] (Started: ${agent.startTime.toISOString()})`);
  }

  console.log('\n--- Scenario 1: Simple Task ---');
  const result1 = await orchestrator.executeTask('Hello, who are you?');
  console.log('Result 1:', result1);

  console.log('\n--- Scenario 2: Task with Delegation (Research) ---');
  const result2 = await orchestrator.executeTask('I need some research on Model Context Protocol.');
  console.log('Result 2:', result2);

  console.log('\n--- Demo Completed ---');
}

main().catch(console.error);
