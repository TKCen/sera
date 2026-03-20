import { Router } from 'express';
import { AgentRegistry } from '../agents/registry.service.js';
import { ResourceImporter } from '../agents/importer.service.js';
import { AgentInstanceSchema } from '../agents/schemas.js';

export function createRegistryRouter(registry: AgentRegistry, importer: ResourceImporter) {
  const router = Router();

  // Templates
  router.get('/templates', async (req, res) => {
    try {
      const templates = await registry.listTemplates();
      res.json(templates);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  router.post('/templates', async (req, res) => {
    try {
      if (req.body.metadata?.builtin) {
        return res.status(403).json({ error: 'Cannot create builtin templates via API' });
      }
      const template = await registry.upsertTemplate(req.body);
      res.status(201).json(template);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  router.get('/templates/:name', async (req, res) => {
    try {
      const template = await registry.getTemplate(req.params.name);
      if (!template) return res.status(404).json({ error: 'Template not found' });
      res.json(template);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  router.put('/templates/:name', async (req, res) => {
    try {
      const template = await registry.updateTemplate(req.params.name, req.body);
      res.json(template);
    } catch (err: any) {
      const code = err.message.includes('not found')
        ? 404
        : err.message.includes('builtin')
          ? 403
          : 500;
      res.status(code).json({ error: err.message });
    }
  });

  router.delete('/templates/:name', async (req, res) => {
    try {
      const template = await registry.deleteTemplate(req.params.name);
      res.json({ message: 'Template deleted', template });
    } catch (err: any) {
      const code = err.message.includes('not found')
        ? 404
        : err.message.includes('builtin')
          ? 403
          : err.message.includes('referenced')
            ? 409
            : 500;
      res.status(code).json({ error: err.message });
    }
  });

  router.get('/templates/:name/instances', async (req, res) => {
    try {
      const instances = await registry.listInstances();
      const filtered = instances.filter((i) => i.template_ref === req.params.name);
      res.json(filtered);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // Instances
  router.get('/instances', async (req, res) => {
    try {
      const instances = await registry.listInstances();
      res.json(instances);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
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
        displayName: metadata.displayName || undefined,
        circle: metadata.circle || undefined,
        overrides: overrides || {},
      } as any);
      res.status(201).json(instance);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  router.get('/instances/:id', async (req, res) => {
    try {
      const instance = await registry.getInstance(req.params.id);
      if (!instance) return res.status(404).json({ error: 'Instance not found' });
      res.json(instance);
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  // Reload
  router.post('/reload', async (req, res) => {
    try {
      await importer.importAll();
      res.json({ status: 'ok', message: 'Registry reloaded from filesystem' });
    } catch (err: any) {
      res.status(500).json({ error: err.message });
    }
  });

  return router;
}
