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

  /**
   * GET /api/agent-templates/:name
   * Retrieves a single agent template manifest.
   */
  router.get('/:name', async (req, res) => {
    const { name } = req.params;
    const filePath = findManifestFile(agentsDir, name);

    if (!filePath) {
      return res.status(404).json({ error: `Agent template "${name}" not found.` });
    }

    try {
      const manifest = AgentManifestLoader.loadManifest(filePath);
      res.json(manifest);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  /**
   * PUT /api/agent-templates/:name
   * Updates an existing agent template manifest on disk.
   */
  router.put('/:name', async (req, res) => {
    try {
      const { name } = req.params;
      const manifest = req.body;

      // Basic validation
      AgentManifestLoader.validateManifest(manifest, `PUT /api/agent-templates/${name}`);

      // Ensure metadata.name matches the URL param
      if (manifest.metadata.name !== name) {
        return res.status(400).json({
          error: `Manifest metadata.name "${manifest.metadata.name}" does not match URL parameter "${name}"`,
        });
      }

      const filePath = findManifestFile(agentsDir, name);
      if (!filePath) {
        return res.status(404).json({ error: `Agent template "${name}" not found.` });
      }

      // Serialize to YAML and overwrite
      const yamlStr = yaml.dump(manifest, { lineWidth: 120, noRefs: true, sortKeys: false });
      fs.writeFileSync(filePath, yamlStr, 'utf-8');

      // Live reload
      orchestrator.reloadTemplates();

      res.json({ success: true });
    } catch (err: any) {
      logger.error('Failed to update agent template:', err);
      res.status(err.name === 'ManifestValidationError' ? 400 : 500).json({ error: err.message });
    }
  });

  /**
   * DELETE /api/agent-templates/:name
   * Deletes an agent template from disk and reloads orchestrator.
   */
  router.delete('/:name', async (req, res) => {
    try {
      const { name } = req.params;
      const filePath = findManifestFile(agentsDir, name);

      if (!filePath) {
        return res.status(404).json({ error: `Agent template "${name}" not found.` });
      }

      // Absolute path safety check (prevent directory traversal)
      const absoluteAgentsDir = path.resolve(agentsDir);
      const absoluteFilePath = path.resolve(filePath);
      if (!absoluteFilePath.startsWith(absoluteAgentsDir)) {
        return res.status(403).json({ error: 'Access denied: manifest is outside agents directory' });
      }

      // Determine if we should delete a directory or just the file
      const fileName = path.basename(filePath);
      if (fileName === 'AGENT.yaml') {
        const parentDir = path.dirname(filePath);
        // Safety check: only delete if the directory name matches the agent name
        if (path.basename(parentDir) === name) {
          fs.rmSync(parentDir, { recursive: true, force: true });
          logger.info(`Deleted agent template directory: ${parentDir}`);
        } else {
          fs.unlinkSync(filePath);
          logger.info(`Deleted agent template file: ${filePath}`);
        }
      } else {
        fs.unlinkSync(filePath);
        logger.info(`Deleted agent template file: ${filePath}`);
      }

      // Live reload
      orchestrator.reloadTemplates();

      res.json({ success: true });
    } catch (err: any) {
      logger.error('Failed to delete agent template:', err);
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}

/**
 * Find the YAML manifest file for a given agent name.
 */
function findManifestFile(agentsDir: string, agentName: string): string | undefined {
  if (!fs.existsSync(agentsDir)) return undefined;

  const entries = fs.readdirSync(agentsDir, { withFileTypes: true });
  for (const entry of entries) {
    let filePath: string | undefined;

    if (entry.isFile() && entry.name.endsWith('.agent.yaml')) {
      filePath = path.join(agentsDir, entry.name);
    } else if (entry.isDirectory()) {
      const subDirAgentFile = path.join(agentsDir, entry.name, 'AGENT.yaml');
      if (fs.existsSync(subDirAgentFile)) {
        filePath = subDirAgentFile;
      }
    }

    if (filePath) {
      try {
        const manifest = AgentManifestLoader.loadManifest(filePath);
        if (manifest.metadata.name === agentName) {
          return filePath;
        }
      } catch {
        // Skip invalid manifests
      }
    }
  }

  return undefined;
}
