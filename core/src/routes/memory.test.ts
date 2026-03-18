import { describe, it, expect, vi, beforeAll } from 'vitest';
import request from 'supertest';
import path from 'path';
import fs from 'fs/promises';
import { app, startServer } from '../index.js';

vi.mock('../services/vector.service.js', () => ({
  VectorService: class {
    async search() { return []; }
    async upsertPoints() {}
    async deletePoints() {}
    async ensureCollection() {}
  }
}));

describe('Memory API', () => {
  beforeAll(async () => {
    // startServer is needed to register routes and init everything
    // but in test env it skips app.listen
    await startServer();
  });

  it('GET /api/memory/blocks should return blocks', async () => {
    const response = await request(app).get('/api/memory/blocks');
    expect(response.status).toBe(200);
    expect(Array.isArray(response.body)).toBe(true);
    // Should have at least the standard block types
    const types = response.body.map((b: any) => b.type);
    expect(types).toContain('human');
    expect(types).toContain('persona');
    expect(types).toContain('core');
    expect(types).toContain('archive');
  });

  it('POST /api/memory/blocks/human should create an entry', async () => {
    const entryData = {
      title: 'Test Entry',
      content: 'This is a test memory entry',
      tags: ['test', 'vitest'],
      source: 'system'
    };
    const response = await request(app)
      .post('/api/memory/blocks/human')
      .send(entryData);

    expect(response.status).toBe(201);
    expect(response.body.title).toBe(entryData.title);
    expect(response.body.content).toBe(entryData.content);
    expect(response.body.type).toBe('human');
    expect(response.body.id).toBeDefined();

    const entryId = response.body.id;

    // Verify we can GET it
    const getRes = await request(app).get(`/api/memory/entries/${entryId}`);
    expect(getRes.status).toBe(200);
    expect(getRes.body.id).toBe(entryId);

    // Verify we can PUT it
    const updateRes = await request(app)
      .put(`/api/memory/entries/${entryId}`)
      .send({ content: 'Updated content' });
    expect(updateRes.status).toBe(200);
    expect(updateRes.body.content).toBe('Updated content');

    // Verify search
    const searchRes = await request(app).get('/api/memory/search?query=Updated');
    expect(searchRes.status).toBe(200);
    expect(searchRes.body.some((e: any) => e.id === entryId)).toBe(true);

    // Verify DELETE
    const delRes = await request(app).delete(`/api/memory/entries/${entryId}`);
    expect(delRes.status).toBe(200);
    expect(delRes.body.success).toBe(true);

    // Verify it's gone
    const getGoneRes = await request(app).get(`/api/memory/entries/${entryId}`);
    expect(getGoneRes.status).toBe(404);
  });
});
