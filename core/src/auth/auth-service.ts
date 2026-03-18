import type { Request } from 'express';
import type { AuthPlugin, OperatorIdentity } from './interfaces.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('AuthService');

export class AuthService {
  private plugins: AuthPlugin[] = [];

  registerPlugin(plugin: AuthPlugin) {
    this.plugins.push(plugin);
    logger.info(`Registered auth plugin: ${plugin.name}`);
  }

  async authenticate(req: Request): Promise<OperatorIdentity | null> {
    for (const plugin of this.plugins) {
      try {
        const identity = await plugin.authenticate(req);
        if (identity) {
          return identity;
        }
      } catch (err: any) {
        // If a plugin recognized its credentials but they were invalid,
        // it should throw. We bubble that up.
        throw err;
      }
    }

    return null;
  }
}
