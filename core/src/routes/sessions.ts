/**
 * Session API Router — CRUD endpoints for chat sessions.
 */

import { Router } from 'express';
import type { SessionStore } from '../sessions/SessionStore.js';

export function createSessionRouter(sessionStore: SessionStore): Router {
  const router = Router();

  /**
   * List sessions, optionally filtered by agent.
   * GET /api/sessions?agent=architect-prime
   */
  router.get('/', async (req, res) => {
    try {
      const agent = req.query.agent as string | undefined;
      const agentInstanceId = req.query.agentInstanceId as string | undefined;
      const sessions = await sessionStore.listSessions(agent, agentInstanceId);
      res.json(sessions);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * Create a new session.
   * POST /api/sessions { agentName: string, title?: string }
   */
  router.post('/', async (req, res) => {
    try {
      const { agentName, title } = req.body;
      if (!agentName) {
        return res.status(400).json({ error: 'agentName is required' });
      }
      const session = await sessionStore.createSession({ agentName, title });
      res.status(201).json(session);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * Get a session with its messages.
   * GET /api/sessions/:id
   */
  router.get('/:id', async (req, res) => {
    try {
      const session = await sessionStore.getSession(req.params.id);
      if (!session) {
        return res.status(404).json({ error: 'Session not found' });
      }
      const messages = await sessionStore.getMessages(req.params.id);
      res.json({ ...session, messages });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * Update session title.
   * PUT /api/sessions/:id { title: string }
   */
  router.put('/:id', async (req, res) => {
    try {
      const { title } = req.body;
      if (!title) {
        return res.status(400).json({ error: 'title is required' });
      }
      const session = await sessionStore.updateSessionTitle(req.params.id, title);
      if (!session) {
        return res.status(404).json({ error: 'Session not found' });
      }
      res.json(session);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * Delete a session and its messages.
   * DELETE /api/sessions/:id
   */
  router.delete('/:id', async (req, res) => {
    try {
      const deleted = await sessionStore.deleteSession(req.params.id);
      if (!deleted) {
        return res.status(404).json({ error: 'Session not found' });
      }
      res.json({ success: true });
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  /**
   * Export a session as markdown or JSON.
   * GET /api/sessions/:id/export?format=markdown|json
   */
  router.get('/:id/export', async (req, res) => {
    try {
      const session = await sessionStore.getSession(req.params.id);
      if (!session) {
        return res.status(404).json({ error: 'Session not found' });
      }
      const messages = await sessionStore.getMessages(req.params.id);
      const format = (req.query.format as string) || 'markdown';

      if (format === 'json') {
        res.setHeader('Content-Type', 'application/json');
        res.setHeader(
          'Content-Disposition',
          `attachment; filename="session-${req.params.id}.json"`
        );
        return res.json({ ...session, messages });
      }

      // Markdown export
      const lines: string[] = [
        `# ${session.title || 'Untitled Session'}`,
        '',
        `**Agent:** ${session.agentName}  `,
        `**Created:** ${session.createdAt}  `,
        `**Messages:** ${messages.length}`,
        '',
        '---',
        '',
      ];

      for (const msg of messages) {
        const role = msg.role === 'user' ? 'You' : msg.role === 'assistant' ? 'Agent' : msg.role;
        lines.push(`### ${role}`);
        lines.push('');
        lines.push(msg.content || '*(empty)*');
        lines.push('');
      }

      const markdown = lines.join('\n');
      res.setHeader('Content-Type', 'text/markdown; charset=utf-8');
      res.setHeader('Content-Disposition', `attachment; filename="session-${req.params.id}.md"`);
      res.send(markdown);
    } catch (err: unknown) {
      res.status(500).json({ error: (err as Error).message });
    }
  });

  return router;
}
