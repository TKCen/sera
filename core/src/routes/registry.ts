import { Router } from 'express';
import { AgentRegistry } from '../agents/index.js';
import { ResourceImporter } from '../agents/index.js';
import { AgentInstanceSchema } from '../agents/index.js';
import type { Orchestrator } from '../agents/index.js';

export function createRegistryRouter(
  registry: AgentRegistry,
  importer: ResourceImporter,
  orchestrator: Orchestrator
) {
  const router = Router();

  // Templates
  router.get('/templates', async (req, res) => {
    try {
      const templates = await registry.listTemplates();
      res.json(templates);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.post('/templates', async (req, res) => {
    try {
      const template = await registry.upsertTemplate(
        req.body as import('../agents/schemas.js').AgentTemplate
      );
      res.status(201).json(template);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.get('/templates/:name', async (req, res) => {
    try {
      const template = await registry.getTemplate(req.params.name);
      if (!template) return res.status(404).json({ error: 'Template not found' });
      res.json(template);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.put('/templates/:name', async (req, res) => {
    try {
      const template = await registry.updateTemplate(
        req.params.name,
        req.body as import('../agents/schemas.js').AgentTemplate
      );
      res.json(template);
    } catch (err: unknown) {
      const error = err as Error;
      const code = error.message.includes('not found')
        ? 404
        : error.message.includes('builtin')
          ? 403
          : 500;
      res.status(code).json({ error: error.message });
    }
  });

  router.delete('/templates/:name', async (req, res) => {
    try {
      const template = await registry.deleteTemplate(req.params.name);
      res.json({ message: 'Template deleted', template });
    } catch (err: unknown) {
      const error = err as Error;
      const code = error.message.includes('not found')
        ? 404
        : error.message.includes('builtin')
          ? 403
          : error.message.includes('referenced')
            ? 409
            : 500;
      res.status(code).json({ error: error.message });
    }
  });

  router.get('/templates/:name/instances', async (req, res) => {
    try {
      const instances = await registry.listInstances();
      const filtered = instances.filter((i) => i.template_ref === req.params.name);
      res.json(filtered);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  // Instances
  router.get('/instances', async (req, res) => {
    try {
      const instances = await registry.listInstances();
      res.json(instances);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.post('/instances', async (req, res) => {
    try {
      const result = AgentInstanceSchema.safeParse(req.body);
      if (!result.success) {
        return res.status(400).json({ error: 'Invalid manifest', details: result.error.format() });
      }

      const { metadata, overrides } = result.data;
      const instance = await registry.createInstance({
        name: metadata.name,
        templateRef: metadata.templateRef,
        ...(metadata.displayName ? { displayName: metadata.displayName } : {}),
        ...(metadata.circle ? { circle: metadata.circle } : {}),
        overrides: (overrides as Record<string, unknown>) || {},
      });
      res.status(201).json(instance);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  router.get('/instances/:id', async (req, res) => {
    try {
      const instance = await registry.getInstance(req.params.id);
      if (!instance) return res.status(404).json({ error: 'Instance not found' });
      res.json(instance);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  // Reload
  router.post('/reload', async (req, res) => {
    try {
      const importerResults = await importer.importAll();
      const orchestratorResults = orchestrator.reloadTemplates();

      res.json({
        status: 'ok',
        message: 'Registry and Orchestrator reloaded from filesystem',
        importer: importerResults,
        orchestrator: orchestratorResults,
      });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  return router;
}
