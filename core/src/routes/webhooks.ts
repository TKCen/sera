import { Router, type Request, type Response } from 'express';
import { Logger } from '../lib/logger.js';
import type { WebhooksService } from '../intercom/WebhooksService.js';

const logger = new Logger('WebhookRouter');

/**
 * Webhook Router — Story 9.8 Secure Endpoints
 */
export function createWebhooksRouter(
  webhooksService: WebhooksService,
  authMiddleware?: import('express').RequestHandler
): Router {
  const router = Router();

  /**
   * Register a new webhook (Auth required)
   */
  if (authMiddleware) {
    router.post('/', authMiddleware, async (req: Request, res: Response) => {
      try {
        const { name, urlPath, secret, eventType } = req.body;
        if (!name || !urlPath || !secret || !eventType) {
          return res
            .status(400)
            .json({ error: 'name, urlPath, secret, and eventType are required' });
        }

        const webhook = await webhooksService.createWebhook(name, urlPath, secret, eventType);
        res.status(201).json(webhook);
      } catch (err: unknown) {
        res.status(500).json({ error: (err as Error).message });
      }
    });

    /**
     * GET /api/webhooks
     * List registered webhooks (Auth required)
     */
    router.get('/', authMiddleware, async (req: Request, res: Response) => {
      try {
        const webhooks = await webhooksService.listWebhooks();
        res.json(webhooks);
      } catch (err: unknown) {
        res.status(500).json({ error: (err as Error).message });
      }
    });
  }

  /**
   * PUBLIC POST /api/webhooks/incoming/:slug
   * Trigger for external events
   */
  router.post('/incoming/:slug', async (req: Request, res: Response) => {
    try {
      const { slug } = req.params;
      const signature = req.headers['x-sera-signature'];
      const timestamp = req.headers['x-sera-timestamp'];
      const nonce = req.headers['x-sera-nonce'];

      const rawBody = (req as unknown as { rawBody?: Buffer }).rawBody
        ? (req as unknown as { rawBody: Buffer }).rawBody.toString('utf-8')
        : JSON.stringify(req.body);

      const sig = Array.isArray(signature) ? signature[0] : signature;
      const ts = Array.isArray(timestamp) ? timestamp[0] : timestamp;
      const nc = (Array.isArray(nonce) ? nonce[0] : nonce) || undefined;

      if (typeof sig === 'string' && typeof ts === 'string') {
        await webhooksService.handleIncoming(
          slug as string,
          rawBody,
          sig,
          ts,
          nc as string | undefined
        );
        return res.status(202).json({ status: 'accepted' });
      }

      return res
        .status(401)
        .json({ error: 'Missing or invalid X-Sera-Signature or X-Sera-Timestamp' });
    } catch (err: unknown) {
      const error = err as Error;
      logger.error(`Webhook processing error (${req.params.slug}):`, error.message);
      res.status(error.message.includes('signature') ? 401 : 404).json({ error: error.message });
    }
  });

  return router;
}
