import path from 'path';
import { Orchestrator } from './agents/Orchestrator.js';
import { Logger } from './lib/logger.js';

const logger = new Logger('Demo');

async function main() {
  logger.info('--- Starting SERA Multi-Agent Demo ---');

  const orchestrator = new Orchestrator();
  const agentsDir = path.resolve(import.meta.dirname, '..', '..', 'agents');
  orchestrator.loadTemplates(agentsDir);

  logger.info('\n--- Loaded Agents ---');
  for (const agent of orchestrator.listAgents()) {
    logger.info(
      `  - ${agent.name} [${agent.status}] (Started: ${agent.startTime.toISOString()})`
    );
  }

  logger.info('\n--- Scenario 1: Simple Task ---');
  const result1 = await orchestrator.executeTask('Hello, who are you?');
  logger.info('Result 1:', result1);

  logger.info('\n--- Scenario 2: Task with Delegation (Research) ---');
  const result2 = await orchestrator.executeTask('I need some research on Model Context Protocol.');
  logger.info('Result 2:', result2);

  logger.info('\n--- Demo Completed ---');
}

main().catch((err) => logger.error('Error:', err));
