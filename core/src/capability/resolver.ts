import type { AgentRegistry } from '../agents/registry.service.js';

export class CapabilityEscalationError extends Error {
  constructor(dimension: string, expected: any, actual: any) {
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

  async resolve(instanceId: string): Promise<any> {
    const instance = await this.registry.getInstance(instanceId);
    if (!instance) throw new Error(`Instance ${instanceId} not found`);

    const template = await this.registry.getTemplate(instance.template_ref);
    if (!template) throw new Error(`Template ${instance.template_ref} not found`);

    // Merge template spec with instance overrides
    const spec = this.deepMerge(template.spec, instance.overrides);

    const boundary = await this.registry.getSandboxBoundary(spec.sandboxBoundary);
    if (!boundary) throw new Error(`Boundary ${spec.sandboxBoundary} not found`);

    const policy = spec.policyRef ? await this.registry.getCapabilityPolicy(spec.policyRef) : null;

    // Resolve base allowed capabilities: Boundary ∩ Policy
    const baseCapabilities = await this.resolveEffectiveCapabilities(
      boundary.capabilities,
      policy?.capabilities || {},
      {} // No inline here yet, we'll check it after
    );

    // Resolve inline capabilities and check for escalation
    const finalCapabilities = await this.resolveEffectiveCapabilities(
      boundary.capabilities,
      policy?.capabilities || {},
      spec.capabilities || {}
    );

    // Resolve manifest capabilities fully expanded
    const manifestCapabilities = await this.expandCapabilities(spec.capabilities || {});

    // Explicit escalation check: manifest overrides cannot be broader than base (Boundary ∩ Policy)
    this.verifyNoEscalation(baseCapabilities, manifestCapabilities);

    // Final pass: Always-denied enforcement
    const effectiveCapabilities = await this.applyAlwaysDenied(finalCapabilities);

    return {
      spec,
      resolvedCapabilities: effectiveCapabilities,
    };
  }

  private verifyNoEscalation(base: any, actual: any, path: string = '') {
    if (!actual) return;

    for (const key in actual) {
      const b = base && typeof base === 'object' ? (base as any)[key] : undefined;
      const a = actual[key];
      const currentPath = path ? `${path}.${key}` : key;

      if (b === undefined || b === false) {
        if (a !== undefined && a !== false && a !== null) {
          // Special case for empty arrays or objects - they are not an escalation if base was undefined
          if (Array.isArray(a) && a.length === 0) continue;
          if (typeof a === 'object' && Object.keys(a).length === 0) continue;

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

      if (typeof b === 'object') {
        if (typeof a !== 'object') throw new CapabilityEscalationError(currentPath, b, a);
        this.verifyNoEscalation(b, a, currentPath);
        continue;
      }

      // Scalar values (should mostly be booleans or handled above)
      if (a !== b) throw new CapabilityEscalationError(currentPath, b, a);
    }
  }

  private async applyAlwaysDenied(capabilities: any) {
    const lists = await this.registry.listAlwaysEnforcedNamedLists();
    if (!lists || !lists.length) return capabilities;

    const result = { ...capabilities };

    // Group by dimension (network-denylist -> network, command-denylist -> exec.commands)
    const denylistsByDimension: Record<string, string[]> = {};
    for (const list of lists) {
      const type = list.type;
      let dimension = '';
      if (type === 'network-denylist') dimension = 'network.outbound';
      if (type === 'command-denylist') dimension = 'exec.commands';

      if (dimension) {
        if (!denylistsByDimension[dimension]) denylistsByDimension[dimension] = [];
        const expanded = await this.expandList(list?.entries || [], type);
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
      let target: any = result;
      for (let i = 0; i < keys.length - 1; i++) {
        const key = keys[i];
        if (key) {
          if (!target[key]) target[key] = {};
          target = target[key];
        }
      }
      const lastKey = keys[keys.length - 1];

      // If the target is an allowlist (array), filter out items that match ANY deny pattern
      if (lastKey && Array.isArray(target[lastKey])) {
        const allowed = target[lastKey] as string[];
        target[lastKey] = allowed.filter(
          (item) => !denyPatterns.some((pattern) => this.matches(item, pattern))
        );
      } else if (lastKey && target[lastKey] === true) {
        // If the target was 'true' (all allowed) but we have a denylist, we must convert to empty (most restrictive safely)
        // or a specific structure. Decision: Always-denied on a 'true' permission means we restrict to 'true' minus those.
        // But for now, if it's 'true', we don't have an allowlist to filter.
        // This is a rare edge case in our current schema.
      }
    }

    return result;
  }

  private matches(value: string, pattern: string): boolean {
    // Glob-style matching: git * matches git status
    // Simple implementation for now: replace * with .* and use regex
    const regex = new RegExp(
      '^' + pattern.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*') + '$'
    );
    return regex.test(value);
  }

  private async resolveEffectiveCapabilities(boundary: any, policy: any, inline: any) {
    const result: any = {};
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

  private async intersect(b: any, p: any, i: any, key: string): Promise<any> {
    // If any layer is missing, it's treated as "not granted" (restricted)
    // UNLESS it's the boundary, which is the ceiling.

    // Boundary is the hard ceiling. If not in boundary, it's false/empty.
    if (b === undefined || b === false) return false;
    if (b === true) {
      // If boundary allows all, policy and inline can restrict
      const base = p !== undefined ? p : b; // Policy wins over boundary if present
      return i !== undefined ? this.narrow(base, i) : base;
    }

    // If boundary is a specific object/list, policy and inline must be sub-sets
    if (typeof b === 'object' && !Array.isArray(b)) {
      const res: any = {};
      const subKeys = new Set([
        ...Object.keys(b),
        ...Object.keys(p || {}),
        ...Object.keys(i || {}),
      ]);
      for (const skey of subKeys) {
        res[skey] = await this.intersect(b[skey], (p || {})[skey], (i || {})[skey], skey);
      }
      return res;
    }

    if (Array.isArray(b)) {
      // For lists (allow/deny), intersection = elements present in ALL layers (if present)
      let current = await this.expandList(b, key);
      if (p !== undefined && Array.isArray(p)) {
        const pExpanded = await this.expandList(p, key);
        const setP = new Set(pExpanded);
        current = current.filter((item) => setP.has(item));
      } else if (p !== undefined) {
        // Policy mismatch, treat as empty (most restrictive)
        current = [];
      }

      if (i !== undefined && Array.isArray(i)) {
        const iExpanded = await this.expandList(i, key);
        const setI = new Set(iExpanded);
        current = current.filter((item) => setI.has(item));
      } else if (i !== undefined) {
        // Inline mismatch, treat as empty
        current = [];
      }
      return current;
    }

    return b; // Default to boundary if no further restriction
  }

  private narrow(base: any, overrides: any) {
    if (base === true) return overrides;
    if (base === false) return false;
    // Further narrowing logic for objects/arrays
    return overrides; // Simplified for now, most restrictive wins
  }

  private async expandCapabilities(caps: any): Promise<any> {
    if (!caps || typeof caps !== 'object') return caps;
    if (Array.isArray(caps)) {
      return this.expandList(caps, 'generic');
    }
    const result: any = {};
    for (const key in caps) {
      result[key] = await this.expandCapabilities(caps[key]);
    }
    return result;
  }

  private async expandList(
    items: any[],
    type: string,
    visited = new Set<string>()
  ): Promise<string[]> {
    if (!Array.isArray(items)) return [];
    const result = new Set<string>();
    for (const item of items) {
      if (typeof item === 'string') {
        result.add(item);
      } else if (item?.$ref) {
        const refName = item.$ref;
        if (visited.has(refName)) {
          throw new Error(`Circular reference detected in NamedList: ${refName}`);
        }
        const nextVisited = new Set(visited);
        nextVisited.add(refName);
        const list = await this.registry.getNamedList(refName);
        if (!list) throw new Error(`NamedList ${refName} not found`);
        const expanded = await this.expandList(list.entries, type, nextVisited);
        expanded.forEach((e) => result.add(e));
      }
    }
    return Array.from(result);
  }

  private deepMerge(base: any, overrides: any): any {
    if (!overrides) return base;

    // If overrides is an array, check for $append/$remove patterns
    if (Array.isArray(overrides)) {
      return overrides; // Standard replacement for arrays unless it's the special skill object
    }

    const result = { ...base };
    for (const key in overrides) {
      const val = overrides[key];

      // Special handling for skill-like lists if they use the $append/$remove pattern
      if (val && typeof val === 'object' && !Array.isArray(val) && (val.$append || val.$remove)) {
        let current = Array.isArray(base[key]) ? [...base[key]] : [];
        if (val.$append) {
          const toAdd = Array.isArray(val.$append) ? val.$append : [val.$append];
          toAdd.forEach((item: any) => {
            if (!current.includes(item)) current.push(item);
          });
        }
        if (val.$remove) {
          const toRemove = Array.isArray(val.$remove) ? val.$remove : [val.$remove];
          current = current.filter((item: any) => !toRemove.includes(item));
        }
        result[key] = current;
      } else if (val && typeof val === 'object' && !Array.isArray(val)) {
        result[key] = this.deepMerge(base[key] || {}, val);
      } else {
        result[key] = val;
      }
    }
    return result;
  }
}
