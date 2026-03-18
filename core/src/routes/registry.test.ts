import { describe, it, expect, vi, beforeEach } from 'vitest';
import request from 'supertest';
import express from 'express';
import { createRegistryRouter } from './registry.js';
import { AgentRegistry } from '../agents/registry.service.js';
import { ResourceImporter } from '../agents/importer.service.js';

describe('Registry Routes', () => {
  let app: any;
  let registryMock: any;
  let importerMock: any;

  beforeEach(() => {
    registryMock = {
      listTemplates: vi.fn(),
      getTemplate: vi.fn(),
      listInstances: vi.fn(),
      createInstance: vi.fn(),
      getInstance: vi.fn(),
    };
    importerMock = {
      importAll: vi.fn(),
    };
    app = express();
    app.use(express.json());
    app.use('/api/registry', createRegistryRouter(registryMock, importerMock));
  });

  it('GET /api/registry/templates returns list', async () => {
    registryMock.listTemplates.mockResolvedValue([{ name: 't1' }]);
    const res = await request(app).get('/api/registry/templates');
    expect(res.status).toBe(200);
    expect(res.body).toEqual([{ name: 't1' }]);
  });

  it('POST /api/registry/instances creates and returns instance', async () => {
    const manifest = {
      apiVersion: 'sera/v1',
      kind: 'Agent',
      metadata: {
        name: 'my-agent',
        templateRef: 'base-template'
      }
    };
    registryMock.createInstance.mockResolvedValue({ id: 'uuid-1', ...manifest.metadata });
    
    const res = await request(app)
      .post('/api/registry/instances')
      .send(manifest);
    
    expect(res.status).toBe(201);
    expect(res.body.id).toBe('uuid-1');
    expect(registryMock.createInstance).toHaveBeenCalledWith({
      name: 'my-agent',
      templateRef: 'base-template',
      displayName: undefined,
      circle: undefined,
      overrides: undefined
    });
  });

  it('POST /api/registry/reload triggers importer', async () => {
    importerMock.importAll.mockResolvedValue(undefined);
    const res = await request(app).post('/api/registry/reload');
    expect(res.status).toBe(200);
    expect(importerMock.importAll).toHaveBeenCalled();
  });

  it('POST /api/registry/instances returns 400 for invalid manifest', async () => {
    const res = await request(app)
      .post('/api/registry/instances')
      .send({ invalid: 'field' });
    expect(res.status).toBe(400);
    expect(res.body.error).toBe('Invalid manifest');
  });
});
