import type { AgentRegistry } from '../agents/registry.service.js';

/**
 * Application-level capability dimensions that bypass sandbox escalation checks.
 * These control what the agent can do within SERA's API (management, delegation, etc.),
 * NOT what the container can do at the OS/network level. They are orthogonal to
 * SandboxBoundary capabilities and should pass through without restriction from tiers.
 */
const APPLICATION_LEVEL_CAPABILITIES = new Set(['seraManagement', 'delegation']);

export class CapabilityEscalationError extends Error {
  constructor(dimension: string, expected: unknown, actual: unknown) {
    const expectedStr = JSON.stringify(expected);
    const actualStr = JSON.stringify(actual);
    super(
      `Capability escalation detected in dimension: ${dimension}. Manifest broadening is not permitted. Expected: ${expectedStr}, Actual: ${actualStr}`
    );
    this.name = 'CapabilityEscalationError';
  }
}

export class CapabilityResolver {
  constructor(private registry: AgentRegistry) {}

  async resolve(instanceId: string): Promise<{ spec: unknown; resolvedCapabilities: unknown }> {
    const instance = await this.registry.getInstance(instanceId);
    if (!instance) throw new Error(`Instance ${instanceId} not found`);

    const template = await this.registry.getTemplate(instance.template_ref);
    if (!template) throw new Error(`Template ${instance.template_ref} not found`);

    // Merge baseline snapshot with instance overrides.
    // If resolved_config is missing (should not happen after migration), fall back to template.
    const baseline = instance.resolved_config ?? template.spec;
    const spec = this.deepMerge(baseline, instance.overrides) as {
      sandboxBoundary: string;
      policyRef?: string;
      capabilities?: Record<string, unknown>;
    };

    const boundary = await this.registry.getSandboxBoundary(spec.sandboxBoundary);
    if (!boundary) throw new Error(`Boundary ${spec.sandboxBoundary} not found`);

    const policy = spec.policyRef ? await this.registry.getCapabilityPolicy(spec.policyRef) : null;

    // Resolve base allowed capabilities: Boundary ∩ Policy
    const baseCapabilities = await this.resolveEffectiveCapabilities(
      (boundary.capabilities as Record<string, unknown>) || {},
      (policy?.capabilities as Record<string, unknown>) || {},
      {} // No inline here yet, we'll check it after
    );

    // Resolve inline capabilities and check for escalation
    const finalCapabilities = await this.resolveEffectiveCapabilities(
      (boundary.capabilities as Record<string, unknown>) || {},
      (policy?.capabilities as Record<string, unknown>) || {},
      spec.capabilities || {}
    );

    // Resolve manifest capabilities fully expanded
    const manifestCapabilities = await this.expandCapabilities(spec.capabilities || {});

    // Explicit escalation check: manifest overrides cannot be broader than base (Boundary ∩ Policy).
    // Application-level capabilities (seraManagement, etc.) are orthogonal to sandbox boundaries
    // and bypass the escalation check — they control what the agent can do within SERA's API,
    // not what the container can do at the OS level.
    const sandboxManifestCaps: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(
      (manifestCapabilities as Record<string, unknown>) || {}
    )) {
      if (!APPLICATION_LEVEL_CAPABILITIES.has(key)) {
        sandboxManifestCaps[key] = value;
      }
    }
    this.verifyNoEscalation(baseCapabilities, sandboxManifestCaps);

    // Merge application-level capabilities into final result (bypass sandbox intersection)
    const appLevelCaps: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(
      (manifestCapabilities as Record<string, unknown>) || {}
    )) {
      if (APPLICATION_LEVEL_CAPABILITIES.has(key)) {
        appLevelCaps[key] = value;
      }
    }
    const mergedCapabilities = {
      ...((finalCapabilities as Record<string, unknown>) || {}),
      ...appLevelCaps,
    };

    // Final pass: Always-denied enforcement
    const effectiveCapabilities = await this.applyAlwaysDenied(mergedCapabilities);

    // ── Populate structured capability fields from boundary + template ────
    // SandboxManager expects these standard fields for container configuration.
    // The intersection logic above handles list/boolean capabilities but doesn't
    // produce the structured fields that SandboxManager reads.
    const resolved = (effectiveCapabilities as Record<string, unknown>) || {};

    // Filesystem: default to write=true unless boundary explicitly restricts
    if (!resolved['filesystem']) {
      resolved['filesystem'] = { write: true };
    }

    // Network outbound: populate from network-allowlist if present
    const allowlist = resolved['network-allowlist'] as string[] | undefined;
    if (allowlist && allowlist.length > 0 && !resolved['network']) {
      resolved['network'] = { outbound: allowlist };
    } else if (!resolved['network']) {
      // No allowlist defined — agents still need internal network
      resolved['network'] = { outbound: [] };
    }

    // Resources: populate from template spec.resources
    const templateSpec = template.spec as Record<string, unknown> | undefined;
    const templateResources = templateSpec?.resources as Record<string, unknown> | undefined;
    if (templateResources && !resolved['resources']) {
      const cpuStr = templateResources.cpu as string | undefined;
      const memStr = templateResources.memory as string | undefined;
      const cpuShares = cpuStr ? Math.round(parseFloat(cpuStr) * 1024) : 0;
      const memoryMB = memStr
        ? memStr.endsWith('Gi')
          ? parseFloat(memStr) * 1024
          : memStr.endsWith('Mi')
            ? parseFloat(memStr)
            : 0
        : 0;
      resolved['resources'] = {
        cpu_shares: cpuShares,
        memory_limit: memoryMB,
      };
    }

    // Linux capabilities from boundary spec.linux
    const boundarySpec = boundary.spec as Record<string, unknown> | undefined;
    const linuxSpec = boundarySpec?.linux as Record<string, unknown> | undefined;
    if (linuxSpec) {
      if (linuxSpec.capabilities && !resolved['capabilities']) {
        resolved['capabilities'] = linuxSpec.capabilities;
      }
      if (linuxSpec.readOnlyRootfs !== undefined) {
        resolved['security'] = { readonlyRootfs: linuxSpec.readOnlyRootfs };
      }
    }

    // Image allowlist from boundary spec (M2.0 — BYOH image validation)
    if (boundarySpec?.allowedImages) {
      resolved['allowedImages'] = boundarySpec.allowedImages;
    }

    return {
      spec,
      resolvedCapabilities: resolved,
    };
  }

  private verifyNoEscalation(base: unknown, actual: unknown, path: string = '') {
    if (!actual || typeof actual !== 'object') return;

    const actualObj = actual as Record<string, unknown>;
    const baseObj = (base && typeof base === 'object' ? base : {}) as Record<string, unknown>;

    for (const key in actualObj) {
      const b = baseObj[key];
      const a = actualObj[key];
      const currentPath = path ? `${path}.${key}` : key;

      if (b === undefined || b === false) {
        if (a !== undefined && a !== false && a !== null) {
          // Special case for empty arrays or objects - they are not an escalation if base was undefined
          if (Array.isArray(a) && a.length === 0) continue;
          if (typeof a === 'object' && a !== null && Object.keys(a).length === 0) continue;

          throw new CapabilityEscalationError(currentPath, b, a);
        }
        continue;
      }

      if (b === true) continue; // Base allowed everything, no escalation possible here

      if (Array.isArray(b)) {
        if (!Array.isArray(a)) throw new CapabilityEscalationError(currentPath, b, a);
        const setB = new Set(b);
        for (const item of a) {
          if (!setB.has(item)) throw new CapabilityEscalationError(currentPath, b, a);
        }
        continue;
      }

      if (typeof b === 'object' && b !== null) {
        if (typeof a !== 'object' || a === null)
          throw new CapabilityEscalationError(currentPath, b, a);
        this.verifyNoEscalation(b, a, currentPath);
        continue;
      }

      // Scalar values (should mostly be booleans or handled above)
      if (a !== b) throw new CapabilityEscalationError(currentPath, b, a);
    }
  }

  private async applyAlwaysDenied(capabilities: unknown): Promise<unknown> {
    const lists = await this.registry.listAlwaysEnforcedNamedLists();
    if (!lists || !lists.length) return capabilities;

    const result = { ...(capabilities as Record<string, unknown>) };

    // Group by dimension (network-denylist -> network, command-denylist -> exec.commands)
    const denylistsByDimension: Record<string, string[]> = {};
    for (const list of lists) {
      const type = list.type;
      let dimension = '';
      if (type === 'network-denylist') dimension = 'network.outbound';
      if (type === 'command-denylist') dimension = 'exec.commands';

      if (dimension) {
        if (!denylistsByDimension[dimension]) denylistsByDimension[dimension] = [];
        const expanded = await this.expandList((list?.entries as unknown[]) || [], type);
        const dimensionList = denylistsByDimension[dimension];
        if (dimensionList) {
          dimensionList.push(...expanded);
        }
      }
    }

    // Apply denylists
    for (const dimension in denylistsByDimension) {
      const denyPatterns = denylistsByDimension[dimension];
      if (!denyPatterns) continue;

      const keys = dimension.split('.');
      let target: Record<string, unknown> = result;
      for (let i = 0; i < keys.length - 1; i++) {
        const key = keys[i];
        if (key) {
          if (!target[key]) target[key] = {};
          target = target[key] as Record<string, unknown>;
        }
      }
      const lastKey = keys[keys.length - 1];

      // If the target is an allowlist (array), filter out items that match ANY deny pattern
      if (lastKey && Array.isArray(target[lastKey])) {
        const allowed = target[lastKey] as string[];
        target[lastKey] = allowed.filter(
          (item) => !denyPatterns.some((pattern) => this.matches(item, pattern))
        );
      }
    }

    return result;
  }

  private matches(value: string, pattern: string): boolean {
    // Glob-style matching: git * matches git status
    const regex = new RegExp(
      '^' + pattern.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*') + '$'
    );
    return regex.test(value);
  }

  private async resolveEffectiveCapabilities(
    boundary: Record<string, unknown>,
    policy: Record<string, unknown>,
    inline: Record<string, unknown>
  ): Promise<Record<string, unknown>> {
    const result: Record<string, unknown> = {};
    const allKeys = new Set([
      ...Object.keys(boundary),
      ...Object.keys(policy),
      ...Object.keys(inline),
    ]);

    for (const key of allKeys) {
      const b = boundary[key];
      const p = policy[key];
      const i = inline[key];

      // Intersection logic: most restrictive wins
      result[key] = await this.intersect(b, p, i, key);
    }
    return result;
  }

  private async intersect(b: unknown, p: unknown, i: unknown, key: string): Promise<unknown> {
    if (b === undefined || b === false) return false;
    if (b === true) {
      const base = p !== undefined ? p : b;
      return i !== undefined ? this.narrow(base, i) : base;
    }

    if (typeof b === 'object' && b !== null && !Array.isArray(b)) {
      const res: Record<string, unknown> = {};
      const bObj = b as Record<string, unknown>;
      const pObj = (p || {}) as Record<string, unknown>;
      const iObj = (i || {}) as Record<string, unknown>;

      const subKeys = new Set([...Object.keys(bObj), ...Object.keys(pObj), ...Object.keys(iObj)]);
      for (const skey of subKeys) {
        res[skey] = await this.intersect(bObj[skey], pObj[skey], iObj[skey], skey);
      }
      return res;
    }

    if (Array.isArray(b)) {
      let current = await this.expandList(b, key);
      if (p !== undefined && Array.isArray(p)) {
        const pExpanded = await this.expandList(p, key);
        const setP = new Set(pExpanded);
        current = current.filter((item) => setP.has(item));
      } else if (p !== undefined) {
        current = [];
      }

      if (i !== undefined && Array.isArray(i)) {
        const iExpanded = await this.expandList(i, key);
        const setI = new Set(iExpanded);
        current = current.filter((item) => setI.has(item));
      } else if (i !== undefined) {
        current = [];
      }
      return current;
    }

    return b;
  }

  private narrow(base: unknown, overrides: unknown) {
    if (base === true) return overrides;
    if (base === false) return false;
    return overrides;
  }

  private async expandCapabilities(caps: unknown): Promise<unknown> {
    if (!caps || typeof caps !== 'object') return caps;
    if (Array.isArray(caps)) {
      return this.expandList(caps, 'generic');
    }
    const result: Record<string, unknown> = {};
    const capsObj = caps as Record<string, unknown>;
    for (const key in capsObj) {
      result[key] = await this.expandCapabilities(capsObj[key]);
    }
    return result;
  }

  private async expandList(
    items: unknown[],
    type: string,
    visited = new Set<string>()
  ): Promise<string[]> {
    if (!Array.isArray(items)) return [];
    const result = new Set<string>();
    for (const item of items) {
      if (typeof item === 'string') {
        result.add(item);
      } else if (item && typeof item === 'object' && (item as Record<string, unknown>).$ref) {
        const refName = (item as Record<string, unknown>).$ref as string;
        if (visited.has(refName)) {
          throw new Error(`Circular reference detected in NamedList: ${refName}`);
        }
        const nextVisited = new Set(visited);
        nextVisited.add(refName);
        const list = await this.registry.getNamedList(refName);
        if (!list) throw new Error(`NamedList ${refName} not found`);
        const expanded = await this.expandList(
          (list.entries as unknown[]) || [],
          type,
          nextVisited
        );
        expanded.forEach((e) => result.add(e));
      }
    }
    return Array.from(result);
  }

  private deepMerge(base: unknown, overrides: unknown): unknown {
    if (!overrides || typeof overrides !== 'object' || Array.isArray(overrides))
      return overrides ?? base;

    const result = { ...((base as Record<string, unknown>) || {}) };
    const overObj = overrides as Record<string, unknown>;
    for (const key in overObj) {
      const val = overObj[key];

      if (
        val &&
        typeof val === 'object' &&
        !Array.isArray(val) &&
        ((val as Record<string, unknown>).$append || (val as Record<string, unknown>).$remove)
      ) {
        const v = val as { $append?: unknown | unknown[]; $remove?: unknown | unknown[] };
        let current = Array.isArray(result[key]) ? [...(result[key] as unknown[])] : [];
        if (v.$append) {
          const toAdd = Array.isArray(v.$append) ? v.$append : [v.$append];
          toAdd.forEach((item) => {
            if (!current.includes(item)) current.push(item);
          });
        }
        if (v.$remove) {
          const toRemove = Array.isArray(v.$remove) ? v.$remove : [v.$remove];
          current = current.filter((item) => !toRemove.includes(item));
        }
        result[key] = current;
      } else if (val && typeof val === 'object' && !Array.isArray(val)) {
        result[key] = this.deepMerge((result[key] || {}) as Record<string, unknown>, val);
      } else {
        result[key] = val;
      }
    }
    return result;
  }
}
