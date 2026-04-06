import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { SSEClientTransport } from '@modelcontextprotocol/sdk/client/sse.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('MCPClient');

export type MCPTransportType = 'stdio' | 'http';

export interface MCPClientOptions {
  name: string;
  transport: MCPTransportType;
  command?: string; // for stdio
  args?: string[]; // for stdio
  url?: string; // for http/sse
  /** Maximum reconnection attempts. Default: Infinity */
  maxReconnectAttempts?: number;
}

export class MCPClient {
  private client: Client;
  private transport: StdioClientTransport | SSEClientTransport;
  private options: MCPClientOptions;

  private connected = false;
  private manuallyClosed = false;
  private reconnectAttempts = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private stopping = false;

  private onReconnectHooks: (() => void)[] = [];

  constructor(options: MCPClientOptions) {
    this.options = options;
    this.transport = this.createTransport();
    this.client = this.createClient();
  }

  private createTransport(): StdioClientTransport | SSEClientTransport {
    if (this.options.transport === 'stdio') {
      if (!this.options.command) throw new Error('Command required for stdio transport');
      return new StdioClientTransport({
        command: this.options.command,
        args: this.options.args ?? [],
      });
    } else {
      if (!this.options.url) throw new Error('URL required for http transport');
      return new SSEClientTransport(new URL(this.options.url));
    }
  }

  private createClient(): Client {
    return new Client(
      {
        name: 'sera-core',
        version: '1.0.0',
      },
      {
        capabilities: {},
      }
    );
  }

  private attachDisconnectHandler(): void {
    this.transport.onclose = () => {
      if (this.manuallyClosed || this.stopping) return;
      this.connected = false;
      logger.warn(`MCP server "${this.options.name}" disconnected — starting reconnection loop`);
      this.scheduleReconnect();
    };

    this.transport.onerror = (error: Error) => {
      logger.error(`MCP transport error for "${this.options.name}":`, error);
      // onclose will fire after onerror for fatal errors; no double-reconnect needed
    };
  }

  private scheduleReconnect(): void {
    const maxAttempts = this.options.maxReconnectAttempts ?? Infinity;
    if (this.reconnectAttempts >= maxAttempts) {
      logger.error(
        `MCP server "${this.options.name}" exceeded max reconnect attempts (${maxAttempts}), giving up`
      );
      return;
    }

    const initialDelay = 1000;
    const maxDelay = 60_000;
    const backoffFactor = 2;
    const delay = Math.min(
      initialDelay * Math.pow(backoffFactor, this.reconnectAttempts),
      maxDelay
    );
    this.reconnectAttempts++;

    logger.info(
      `MCP server "${this.options.name}": reconnect attempt ${this.reconnectAttempts} in ${delay}ms`
    );

    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      void this.attemptReconnect();
    }, delay);
  }

  private async attemptReconnect(): Promise<void> {
    if (this.manuallyClosed || this.stopping) return;

    try {
      // Recreate transport and client — the SDK does not support reusing after close
      this.transport = this.createTransport();
      this.client = this.createClient();
      this.attachDisconnectHandler();

      await this.client.connect(this.transport);
      this.connected = true;
      this.reconnectAttempts = 0;

      logger.info(`MCP server "${this.options.name}" reconnected successfully`);

      for (const hook of this.onReconnectHooks) {
        try {
          hook();
        } catch (err) {
          logger.error(`onReconnect hook error for "${this.options.name}":`, err);
        }
      }
    } catch (err) {
      logger.error(`MCP server "${this.options.name}": reconnect attempt failed:`, err);
      if (!this.manuallyClosed && !this.stopping) {
        this.scheduleReconnect();
      }
    }
  }

  /** Register a callback invoked after each successful reconnect (e.g. to re-discover tools). */
  public onReconnect(hook: () => void): void {
    this.onReconnectHooks.push(hook);
  }

  async connect() {
    this.manuallyClosed = false;
    this.stopping = false;
    await this.client.connect(this.transport);
    this.connected = true;
    this.attachDisconnectHandler();
    logger.info(`Connected to MCP server "${this.options.name}"`);
  }

  async listTools() {
    return await this.client.listTools();
  }

  async callTool(
    name: string,
    arguments_: Record<string, unknown>,
    meta?: Record<string, unknown>
  ) {
    return await this.client.callTool({
      name,
      arguments: arguments_,
      _meta: meta,
    });
  }

  /** Stop the reconnection loop without closing the transport. */
  public stopReconnecting(): void {
    this.stopping = true;
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }

  get isConnected(): boolean {
    return this.connected;
  }

  async disconnect() {
    this.manuallyClosed = true;
    this.stopReconnecting();
    this.connected = false;
    await this.transport.close();
  }
}
