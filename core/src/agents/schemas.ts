import { z } from 'zod';

// ── Shared ──────────────────────────────────────────────────────────────────

const MetadataSchema = z.object({
  name: z
    .string()
    .regex(/^[a-z0-9]([-a-z0-9]*[a-z0-9])?$/)
    .max(63),
  displayName: z.string().optional(),
  icon: z.string().optional(),
  builtin: z.boolean().default(false),
  category: z.string().optional(),
  description: z.string().optional(),
});

// ── AgentTemplate ───────────────────────────────────────────────────────────

export const AgentTemplateSchema = z.object({
  apiVersion: z.literal('sera/v1'),
  kind: z.literal('AgentTemplate'),
  metadata: MetadataSchema,
  spec: z.object({
    identity: z
      .object({
        role: z.string().optional(),
        principles: z.array(z.string()).optional(),
      })
      .optional(),
    model: z
      .object({
        provider: z.string().optional(),
        name: z.string().optional(),
        temperature: z.number().optional(),
        fallback: z
          .array(
            z.object({
              provider: z.string(),
              name: z.string(),
              maxComplexity: z.number().optional(),
            })
          )
          .optional(),
      })
      .optional(),
    sandboxBoundary: z.string().optional(),
    policyRef: z.string().optional(),
    capabilities: z.record(z.any()).optional(),
    lifecycle: z.object({
      mode: z.enum(['persistent', 'ephemeral']),
    }),
    skills: z.array(z.string()).optional(),
    skillPackages: z.array(z.string()).optional(),
    tools: z
      .object({
        allowed: z.array(z.string()).optional(),
        denied: z.array(z.string()).optional(),
      })
      .optional(),
    subagents: z
      .object({
        allowed: z
          .array(
            z.object({
              templateRef: z.string(),
              maxInstances: z.number().optional(),
              lifecycle: z.enum(['persistent', 'ephemeral']).default('ephemeral'),
              requiresApproval: z.boolean().default(false),
            })
          )
          .optional(),
      })
      .optional(),
    resources: z
      .object({
        cpu: z.string().optional(),
        memory: z.string().optional(),
        maxLlmTokensPerHour: z.number().optional(),
        maxLlmTokensPerDay: z.number().optional(),
      })
      .optional(),
    workspace: z.record(z.any()).optional(),
    memory: z.record(z.any()).optional(),
  }),
});

// ── Agent Instance ──────────────────────────────────────────────────────────

export const AgentInstanceSchema = z.object({
  apiVersion: z.literal('sera/v1'),
  kind: z.literal('Agent'),
  metadata: z.object({
    name: z
      .string()
      .regex(/^[a-z0-9]([-a-z0-9]*[a-z0-9])?$/)
      .max(63),
    displayName: z.string().optional(),
    templateRef: z.string(),
    circle: z.string().optional(),
  }),
  overrides: z.record(z.any()).optional(),
});

// ── NamedList ───────────────────────────────────────────────────────────────

export const NamedListSchema = z.object({
  apiVersion: z.literal('sera/v1'),
  kind: z.literal('NamedList'),
  metadata: z.object({
    name: z.string(),
    type: z.enum([
      'network-allowlist',
      'network-denylist',
      'command-allowlist',
      'command-denylist',
      'secret-list',
    ]),
    description: z.string().optional(),
    alwaysEnforced: z.boolean().default(false),
  }),
  entries: z.array(z.union([z.string(), z.object({ $ref: z.string() })])),
});

// ── CapabilityPolicy ────────────────────────────────────────────────────────

export const CapabilityPolicySchema = z.object({
  apiVersion: z.literal('sera/v1'),
  kind: z.literal('CapabilityPolicy'),
  metadata: z.object({
    name: z.string(),
    description: z.string().optional(),
  }),
  capabilities: z.record(z.any()),
});

// ── SandboxBoundary ─────────────────────────────────────────────────────────

export const SandboxBoundarySchema = z.object({
  apiVersion: z.literal('sera/v1'),
  kind: z.literal('SandboxBoundary'),
  metadata: z.object({
    name: z.string(),
    description: z.string().optional(),
  }),
  spec: z.object({
    linux: z.object({
      capabilities: z.array(z.string()),
      seccomp: z.string().optional(),
      readOnlyRootfs: z.boolean().optional(),
      runAsNonRoot: z.boolean().optional(),
    }),
    capabilities: z.record(z.any()),
  }),
});

export type AgentTemplate = z.infer<typeof AgentTemplateSchema>;
export type AgentInstance = z.infer<typeof AgentInstanceSchema>;
export type NamedList = z.infer<typeof NamedListSchema>;
export type CapabilityPolicy = z.infer<typeof CapabilityPolicySchema>;
export type SandboxBoundary = z.infer<typeof SandboxBoundarySchema>;
