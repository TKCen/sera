import { Router } from 'express';
import fs from 'fs';
import path from 'path';
import yaml from 'js-yaml';
import { AgentManifestLoader } from '../agents/manifest/AgentManifestLoader.js';
import type { Orchestrator } from '../agents/Orchestrator.js';
import { AgentFactory } from '../agents/AgentFactory.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('AgentTemplatesRouter');

export function createAgentTemplatesRouter(orchestrator: Orchestrator, agentsDir: string) {
  const router = Router();

  /**
   * POST /api/agent-templates
   * Creates a new agent template (AGENT.yaml) on disk.
   */
  router.post('/', async (req, res) => {
    try {
      const manifest = req.body;

      // Basic validation
      AgentManifestLoader.validateManifest(manifest, 'POST /api/agent-templates');

      const agentName = manifest.metadata.name;
      const agentDir = path.join(agentsDir, agentName);

      if (fs.existsSync(agentDir)) {
        return res.status(400).json({ error: `Agent template "${agentName}" already exists.` });
      }

      // Create directory and write AGENT.yaml
      fs.mkdirSync(agentDir, { recursive: true });
      const yamlStr = yaml.dump(manifest, { lineWidth: 120, noRefs: true, sortKeys: false });
      fs.writeFileSync(path.join(agentDir, 'AGENT.yaml'), yamlStr, 'utf-8');

      // Live reload
      orchestrator.reloadTemplates();

      res.status(201).json({ success: true, name: agentName });
    } catch (err: any) {
      logger.error('Failed to create agent template:', err);
      res.status(err.name === 'ManifestValidationError' ? 400 : 500).json({ error: err.message });
    }
  });

  /**
   * POST /api/agent-templates/test-chat
   * Temporary "preview" chat with a non-persisted manifest.
   */
  router.post('/test-chat', async (req, res) => {
    try {
      const { manifest, message, history = [] } = req.body;

      if (!manifest || !message) {
        return res.status(400).json({ error: 'manifest and message are required' });
      }

      // Validate manifest
      AgentManifestLoader.validateManifest(manifest, 'POST /api/agent-templates/test-chat');

      // Create a transient agent instance (not registered in Orchestrator's main map)
      const agent = AgentFactory.createAgent(manifest);

      // If orchestrator has a tool executor, attach it
      const toolExecutor = orchestrator.getToolExecutor();
      if (toolExecutor) {
        agent.setToolExecutor(toolExecutor);
      }

      const response = await agent.process(message, history);

      res.json({
        reply: response.finalAnswer || response.thought || 'No response.',
        thought: response.thought
      });
    } catch (err: any) {
      logger.error('Preview chat error:', err);
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}
