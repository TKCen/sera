import type { AgentRegistry } from '../agents/registry.service.js';

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

    // Resolve effective capabilities: Boundary ∩ Policy ∩ ManifestInline
    const effectiveCapabilities = await this.resolveEffectiveCapabilities(
      boundary.capabilities,
      policy?.capabilities || {},
      spec.capabilities || {}
    );

    return {
      spec,
      resolvedCapabilities: effectiveCapabilities,
    };
  }

  private async resolveEffectiveCapabilities(boundary: any, policy: any, inline: any) {
    const result: any = {};
    const allKeys = new Set([
      ...Object.keys(boundary),
      ...Object.keys(policy),
      ...Object.keys(inline)
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
      const subKeys = new Set([...Object.keys(b), ...Object.keys(p || {}), ...Object.keys(i || {})]);
      for (const skey of subKeys) {
        res[skey] = await this.intersect(b[skey], (p || {})[skey], (i || {})[skey], skey);
      }
      return res;
    }

    if (Array.isArray(b)) {
      // For lists (allow/deny), intersection = elements present in ALL layers (if present)
      let current = await this.expandList(b, key);
      if (p !== undefined) {
        const pExpanded = await this.expandList(p, key);
        const setP = new Set(pExpanded);
        current = current.filter(item => setP.has(item));
      }
      if (i !== undefined) {
        const iExpanded = await this.expandList(i, key);
        const setI = new Set(iExpanded);
        current = current.filter(item => setI.has(item));
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

  private async expandList(items: any[], type: string, visited = new Set<string>()): Promise<string[]> {
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
        expanded.forEach(e => result.add(e));
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
