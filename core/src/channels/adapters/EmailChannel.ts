import nodemailer from 'nodemailer';
import type { Transporter } from 'nodemailer';
import { Logger } from '../../lib/logger.js';
import type { Channel, ChannelEvent, ChannelHealth } from '../channel.interface.js';
import { ActionTokenService } from '../ActionTokenService.js';

const logger = new Logger('EmailChannel');

interface EmailConfig {
  smtpHost: string;
  smtpPort: number;
  smtpUser?: string | undefined;
  smtpPassword?: string | undefined;
  from: string;
  to: string[];
}

function renderHtml(event: ChannelEvent, approveUrl?: string, denyUrl?: string): string {
  const severityColor: Record<string, string> = {
    info: '#3b82f6',
    warning: '#f59e0b',
    critical: '#ef4444',
  };
  const color = severityColor[event.severity] ?? '#6b7280';

  const actionButtons =
    approveUrl && denyUrl
      ? `
      <div style="margin-top:24px">
        <a href="${approveUrl}" style="background:${color};color:#fff;padding:10px 20px;border-radius:4px;text-decoration:none;margin-right:12px">✅ Approve</a>
        <a href="${denyUrl}" style="background:#6b7280;color:#fff;padding:10px 20px;border-radius:4px;text-decoration:none">❌ Deny</a>
      </div>`
      : '';

  return `
    <div style="font-family:sans-serif;max-width:600px;margin:0 auto">
      <div style="border-left:4px solid ${color};padding:16px;background:#f9fafb">
        <h2 style="margin:0 0 8px;color:${color}">${event.title}</h2>
        <p style="margin:0;color:#374151">${event.body}</p>
        ${actionButtons}
      </div>
      <p style="color:#9ca3af;font-size:12px;margin-top:16px">
        SERA · ${event.eventType} · ${event.timestamp}
      </p>
    </div>`;
}

function renderText(event: ChannelEvent, approveUrl?: string, denyUrl?: string): string {
  let text = `[SERA] [${event.severity.toUpperCase()}] ${event.title}\n\n${event.body}`;
  if (approveUrl && denyUrl) {
    text += `\n\nApprove: ${approveUrl}\nDeny: ${denyUrl}`;
  }
  return text;
}

export class EmailChannel implements Channel {
  readonly type = 'email';
  private cfg: EmailConfig;
  private transporter: Transporter;

  constructor(
    readonly id: string,
    readonly name: string,
    config: Record<string, unknown>
  ) {
    const smtpUser = config['smtpUser'];
    const smtpPassword = config['smtpPassword'];
    this.cfg = {
      smtpHost: config['smtpHost'] as string,
      smtpPort: typeof config['smtpPort'] === 'number' ? config['smtpPort'] : 587,
      ...(typeof smtpUser === 'string' ? { smtpUser } : {}),
      ...(typeof smtpPassword === 'string' ? { smtpPassword } : {}),
      from: config['from'] as string,
      to: Array.isArray(config['to']) ? (config['to'] as string[]) : [],
    };

    this.transporter = nodemailer.createTransport({
      host: this.cfg.smtpHost,
      port: this.cfg.smtpPort,
      secure: this.cfg.smtpPort === 465,
      requireTLS: true,
      ...(this.cfg.smtpUser && this.cfg.smtpPassword
        ? { auth: { user: this.cfg.smtpUser, pass: this.cfg.smtpPassword } }
        : {}),
    });
  }

  async send(event: ChannelEvent): Promise<void> {
    let approveUrl: string | undefined;
    let denyUrl: string | undefined;

    if (event.actions) {
      const urls = ActionTokenService.getInstance().buildActionUrls(
        event.actions.approveToken,
        event.actions.denyToken
      );
      approveUrl = urls.approveUrl;
      denyUrl = urls.denyUrl;
    }

    const subject = `[SERA] [${event.severity.toUpperCase()}] ${event.title}`;

    await this.transporter.sendMail({
      from: this.cfg.from,
      to: this.cfg.to.join(', '),
      subject,
      html: renderHtml(event, approveUrl, denyUrl),
      text: renderText(event, approveUrl, denyUrl),
    });

    logger.info(`Email sent: ${event.eventType} → ${this.cfg.to.join(', ')}`);
  }

  async healthCheck(): Promise<ChannelHealth> {
    const start = Date.now();
    try {
      await this.transporter.verify();
      return { healthy: true, latencyMs: Date.now() - start };
    } catch (err: unknown) {
      return { healthy: false, error: err instanceof Error ? err.message : String(err) };
    }
  }
}
