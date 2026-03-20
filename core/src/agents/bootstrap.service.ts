import type { AgentRegistry } from './registry.service.js';
import type { ResourceImporter } from './importer.service.js';
import { Logger } from '../lib/logger.js';

const logger = new Logger('Bootstrap');

export class BootstrapService {
  constructor(
    private registry: AgentRegistry,
    private importer: ResourceImporter,
    private workspaceRoot: string
  ) {}

  async ensureSeraInstantiated() {
    const instances = await this.registry.listInstances();
    if (instances.length > 0) {
      return { bootstrapped: true, seraInstanceId: instances.find((i) => i.name === 'sera')?.id };
    }

    logger.info('Performing initial bootstrap...');

    // 1. Ensure built-in resources are imported
    await this.importer.importAll();

    // 2. Load Sera template
    const seraTemplate = await this.registry.getTemplate('sera');
    if (!seraTemplate) {
      throw new Error('Sera template not found in registry after import. Bootstrap failed.');
    }

    // 3. Instantiate Sera
    const sera = await this.registry.createInstance({
      name: 'sera',
      displayName: 'Sera (Primary Agent)',
      templateRef: 'sera',
      circle: 'default',
      lifecycleMode: 'persistent',
    });

    logger.info(`Sera primary agent instantiated with ID: ${sera.id}`);

    return { bootstrapped: true, seraInstanceId: sera.id };
  }

  async getBootstrapStatus() {
    const instances = await this.registry.listInstances();
    const sera = instances.find((i) => i.name === 'sera');
    return {
      bootstrapped: !!sera,
      seraInstanceId: sera?.id || null,
    };
  }
}
