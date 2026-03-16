import path from 'path';
import { LspClient } from './LspClient.js';
import { URI } from 'vscode-uri';

export class LspManager {
  private clients: Map<string, LspClient> = new Map();
  private rootDir: string;

  constructor(rootDir: string) {
    this.rootDir = rootDir;
  }

  private getLanguageServerConfig(ext: string): { command: string, args: string[] } | null {
    switch (ext) {
      case '.ts':
      case '.tsx':
      case '.js':
      case '.jsx':
        return {
          command: 'typescript-language-server',
          args: ['--stdio']
        };
      default:
        return null;
    }
  }

  async getClientForFile(filePath: string): Promise<LspClient | null> {
    const ext = path.extname(filePath);
    const config = this.getLanguageServerConfig(ext);

    if (!config) return null;

    const key = config.command;
    if (this.clients.has(key)) {
      return this.clients.get(key)!;
    }

    const client = new LspClient({
      rootUri: URI.file(this.rootDir).toString(),
      serverCommand: config.command,
      serverArgs: config.args
    });

    await client.start();
    this.clients.set(key, client);
    return client;
  }

  async stopAll(): Promise<void> {
    for (const client of this.clients.values()) {
      await client.stop();
    }
    this.clients.clear();
  }
}
