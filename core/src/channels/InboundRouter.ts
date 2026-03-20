import { pool } from '../lib/database.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('InboundRouter');

interface InboundRoute {
  id: string;
  channelId: string;
  channelType: string;
  platformChannelId: string;
  targetAgentId: string;
  prefix: string | null;
  taskTemplate: string;
}

/** Allowed interpolation variables — no arbitrary expressions. */
function interpolate(template: string, message: string, sender: string): string {
  return template.replace(/\{\{message\}\}/g, message).replace(/\{\{sender\}\}/g, sender);
}

/**
 * Wrap inbound platform message in a SERA delimiter to mark it as
 * untrusted external content before injecting into agent context.
 */
function wrapMessage(channelType: string, text: string): string {
  return `<channel_message source="${channelType}">${text}</channel_message>`;
}

export class InboundRouter {
  private static instance: InboundRouter;

  private constructor() {}

  static getInstance(): InboundRouter {
    if (!InboundRouter.instance) {
      InboundRouter.instance = new InboundRouter();
    }
    return InboundRouter.instance;
  }

  async route(
    channelType: string,
    platformChannelId: string,
    message: string,
    sender: string,
    platformUserId: string,
    dispatchTask: (agentId: string, task: string, context: Record<string, unknown>) => Promise<void>
  ): Promise<boolean> {
    let routes: InboundRoute[] = [];
    try {
      const { rows } = await pool.query<{
        id: string;
        channel_id: string;
        channel_type: string;
        platform_channel_id: string;
        target_agent_id: string;
        prefix: string | null;
        task_template: string;
      }>(
        `SELECT * FROM inbound_channel_routes
         WHERE channel_type = $1 AND platform_channel_id = $2`,
        [channelType, platformChannelId]
      );

      routes = rows.map((r) => ({
        id: r.id,
        channelId: r.channel_id,
        channelType: r.channel_type,
        platformChannelId: r.platform_channel_id,
        targetAgentId: r.target_agent_id,
        prefix: r.prefix,
        taskTemplate: r.task_template,
      }));
    } catch (err) {
      logger.warn('Failed to load inbound routes:', err);
      return false;
    }

    let routed = false;
    for (const route of routes) {
      let msgText = message;

      if (route.prefix) {
        if (!message.startsWith(route.prefix)) continue;
        msgText = message.slice(route.prefix.length).trimStart();
      }

      const wrapped = wrapMessage(channelType, msgText);
      const taskText = interpolate(route.taskTemplate, wrapped, sender);

      const context: Record<string, unknown> = {
        source: 'channel',
        channelType,
        platformUserId,
        platformUsername: sender,
      };

      try {
        await dispatchTask(route.targetAgentId, taskText, context);
        routed = true;
        logger.info(
          `Inbound ${channelType} message from ${sender} routed to agent ${route.targetAgentId}`
        );
      } catch (err) {
        logger.warn(`Failed to dispatch inbound message to agent ${route.targetAgentId}:`, err);
      }
    }

    return routed;
  }
}
