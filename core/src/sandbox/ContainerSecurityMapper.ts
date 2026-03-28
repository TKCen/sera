import type { ResolvedCapabilities } from '../agents/manifest/types.js';

export interface SecurityOptions {
  CpuShares: number;
  Memory: number;
  AutoRemove: boolean;
  CapDrop: string[];
  CapAdd?: string[];
  ReadonlyRootfs: boolean;
}

export class ContainerSecurityMapper {
  static mapSecurityOptions(caps: ResolvedCapabilities, isEphemeral: boolean): SecurityOptions {
    const cpuShares = caps.resources?.cpu_shares || 0;
    const memoryBytes = (caps.resources?.memory_limit || 0) * 1024 * 1024;
    const linuxCaps: string[] = Array.isArray(caps.capabilities) ? caps.capabilities : [];

    const options: SecurityOptions = {
      CpuShares: cpuShares,
      Memory: memoryBytes,
      AutoRemove: isEphemeral,
      CapDrop: ['ALL'],
      ReadonlyRootfs: caps.security?.readonlyRootfs ?? false,
    };

    if (linuxCaps.length > 0) {
      options.CapAdd = [...linuxCaps];
    }

    if (caps.capabilities?.includes('CHOWN')) {
      options.CapAdd = options.CapAdd || [];
      if (!options.CapAdd.includes('CHOWN')) {
        options.CapAdd.push('CHOWN');
      }
    }

    return options;
  }
}
