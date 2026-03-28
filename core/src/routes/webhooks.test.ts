import { describe, it, expect, vi, beforeEach } from 'vitest';
import express from 'express';
import request from 'supertest';
import { createWebhooksRouter } from './webhooks.js';
import type { WebhooksService } from '../intercom/WebhooksService.js';

vi.mock('../lib/logger.js', () => ({
  Logger: class {
    error = vi.fn();
    info = vi.fn();
    warn = vi.fn();
  },
}));

describe('WebhooksRouter', () => {
  let app: express.Express;
  let mockWebhooksService: Partial<WebhooksService>;

  // A simple auth middleware mock
  const mockAuthMiddleware = vi.fn((req, res, next) => {
    next();
  });

  beforeEach(() => {
    vi.clearAllMocks();

    mockWebhooksService = {
      createWebhook: vi.fn(),
      listWebhooks: vi.fn(),
      handleIncoming: vi.fn(),
    };

    app = express();
    // Middleware to optionally set rawBody for testing webhook signatures
    app.use((req, res, next) => {
      let data = '';
      req.on('data', chunk => {
        data += chunk;
      });
      req.on('end', () => {
        if (data) {
          try {
            req.body = JSON.parse(data);
          } catch (e) {
             req.body = {};
          }
          (req as any).rawBody = Buffer.from(data);
        }
        next();
      });
    });

    app.use('/api/webhooks', createWebhooksRouter(mockWebhooksService as WebhooksService, mockAuthMiddleware));
  });

  describe('POST /api/webhooks', () => {
    it('creates a new webhook', async () => {
      const webhookInput = {
        name: 'My Webhook',
        urlPath: '/custom/path',
        secret: 'supersecret',
        eventType: 'issue_created'
      };
      const createdWebhook = { id: 'wh-1', slug: 'my-webhook-slug', ...webhookInput };

      vi.mocked(mockWebhooksService.createWebhook!).mockResolvedValueOnce(createdWebhook as any);

      const res = await request(app).post('/api/webhooks').send(webhookInput);

      expect(res.status).toBe(201);
      expect(res.body).toEqual(createdWebhook);
      expect(mockWebhooksService.createWebhook).toHaveBeenCalledWith('My Webhook', '/custom/path', 'supersecret', 'issue_created');
      expect(mockAuthMiddleware).toHaveBeenCalled();
    });

    it('returns 400 if required fields are missing', async () => {
      const res = await request(app).post('/api/webhooks').send({ name: 'Only Name' });

      expect(res.status).toBe(400);
      expect(res.body.error).toContain('name, urlPath, secret, and eventType are required');
    });

    it('returns 500 on service error', async () => {
      vi.mocked(mockWebhooksService.createWebhook!).mockRejectedValueOnce(new Error('Service failure'));

      const res = await request(app).post('/api/webhooks').send({
        name: 'My Webhook',
        urlPath: '/custom/path',
        secret: 'supersecret',
        eventType: 'issue_created'
      });

      expect(res.status).toBe(500);
      expect(res.body.error).toEqual('Service failure');
    });
  });

  describe('GET /api/webhooks', () => {
    it('lists registered webhooks', async () => {
      const webhooks = [{ id: 'wh-1', name: 'Test' }];
      vi.mocked(mockWebhooksService.listWebhooks!).mockResolvedValueOnce(webhooks as any);

      const res = await request(app).get('/api/webhooks');

      expect(res.status).toBe(200);
      expect(res.body).toEqual(webhooks);
      expect(mockWebhooksService.listWebhooks).toHaveBeenCalled();
      expect(mockAuthMiddleware).toHaveBeenCalled();
    });

    it('returns 500 on service error', async () => {
      vi.mocked(mockWebhooksService.listWebhooks!).mockRejectedValueOnce(new Error('List failure'));

      const res = await request(app).get('/api/webhooks');

      expect(res.status).toBe(500);
      expect(res.body.error).toEqual('List failure');
    });
  });

  describe('POST /api/webhooks/incoming/:slug', () => {
    it('processes a valid incoming webhook event', async () => {
      vi.mocked(mockWebhooksService.handleIncoming!).mockResolvedValueOnce();

      const payload = { action: 'opened' };
      const res = await request(app)
        .post('/api/webhooks/incoming/test-slug')
        .set('x-sera-signature', 'valid-signature')
        .set('x-sera-timestamp', '1234567890')
        .set('x-sera-nonce', 'random-nonce')
        .send(payload);

      expect(res.status).toBe(202);
      expect(res.body).toEqual({ status: 'accepted' });
      expect(mockWebhooksService.handleIncoming).toHaveBeenCalledWith(
        'test-slug',
        JSON.stringify(payload),
        'valid-signature',
        '1234567890',
        'random-nonce'
      );
    });

    it('returns 401 if missing headers', async () => {
      const res = await request(app)
        .post('/api/webhooks/incoming/test-slug')
        .send({});

      expect(res.status).toBe(401);
      expect(res.body.error).toContain('Missing or invalid X-Sera-Signature or X-Sera-Timestamp');
    });

    it('handles signature validation errors by returning 401', async () => {
      vi.mocked(mockWebhooksService.handleIncoming!).mockRejectedValueOnce(new Error('Invalid signature'));

      const res = await request(app)
        .post('/api/webhooks/incoming/test-slug')
        .set('x-sera-signature', 'bad-signature')
        .set('x-sera-timestamp', '1234567890')
        .send({});

      expect(res.status).toBe(401);
      expect(res.body.error).toEqual('Invalid signature');
    });

    it('handles other errors by returning 404', async () => {
      vi.mocked(mockWebhooksService.handleIncoming!).mockRejectedValueOnce(new Error('Webhook not found'));

      const res = await request(app)
        .post('/api/webhooks/incoming/test-slug')
        .set('x-sera-signature', 'valid-signature')
        .set('x-sera-timestamp', '1234567890')
        .send({});

      expect(res.status).toBe(404);
      expect(res.body.error).toEqual('Webhook not found');
    });
  });
});
