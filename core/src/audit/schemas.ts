import { z } from 'zod';

/**
 * Story 11.4: Event payload schemas.
 */

export const AgentSpawnedSchema = z.object({
  agentId: z.string().uuid(),
  agentName: z.string(),
  containerId: z.string(),
});

export const AgentStoppedSchema = z.object({
  agentId: z.string().uuid(),
});

export const AgentCrashedSchema = z.object({
  agentId: z.string().uuid(),
  agentName: z.string(),
  error: z.string().optional(),
  exitCode: z.number().optional(),
});

export const ToolCalledSchema = z.object({
  skillId: z.string().optional(), // From ToolExecutor
  tool: z.string().optional(), // From BaseAgent
  params: z.unknown().optional(),
  args: z.unknown().optional(),
  success: z.boolean().optional(),
  error: z.string().optional(),
  result: z.string().optional(),
});

export const ToolDeniedSchema = z.object({
  skillId: z.string(),
  agentInstanceId: z.string().uuid().optional(),
});

export const PermissionRequestedSchema = z.object({
  requestId: z.string().uuid(),
  dimension: z.string(),
  value: z.string(),
  reason: z.string().optional(),
});

export const PermissionDecisionSchema = z.object({
  requestId: z.string().uuid(),
  agentId: z.string().uuid(),
  dimension: z.string(),
  value: z.string(),
  grantType: z.string().optional(),
});

export const SecretAccessedSchema = z.object({
  secretName: z.string(),
});

export const BudgetExceededSchema = z.object({
  hourlyUsed: z.number(),
  hourlyQuota: z.number(),
  dailyUsed: z.number(),
  dailyQuota: z.number(),
});

export const ScheduleFiredSchema = z.object({
  scheduleId: z.string().uuid(),
  agentId: z.string().uuid(),
});

export const KnowledgeCommittedSchema = z.object({
  blockId: z.string(),
  type: z.string(),
  scope: z.string(),
  circleId: z.string().optional(),
});

export const ApiKeyCreatedSchema = z.object({
  keyId: z.string().uuid(),
  name: z.string(),
  roles: z.array(z.string()),
});

export const ApiKeyRevokedSchema = z.object({
  keyId: z.string().uuid(),
});

export const AuditExportedSchema = z.object({
  format: z.string(),
});

export const CircleCreatedSchema = z.object({
  circleId: z.string().uuid(),
  name: z.string(),
  displayName: z.string(),
});

export const CircleDeletedSchema = z.object({
  circleId: z.string().uuid(),
});

export const CircleMembershipChangedSchema = z.object({
  circleId: z.string().uuid(),
  agentId: z.string().uuid(),
  action: z.enum(['added', 'removed']),
});

export const DelegationCreatedSchema = z.object({
  delegationId: z.string().uuid(),
  agentId: z.string(),
  service: z.string(),
  grantType: z.string(),
});

export const DelegationRevokedSchema = z.object({
  delegationId: z.string().uuid(),
  cascade: z.boolean(),
  childTokensRevoked: z.number(),
  revokedBy: z.string(),
});

export const DelegationDerivedSchema = z.object({
  parentDelegationId: z.string().uuid(),
  childDelegationId: z.string().uuid(),
  childAgentId: z.string(),
  narrowedScope: z.unknown(),
});

export const CredentialResolutionDeniedSchema = z.object({
  service: z.string(),
  agentId: z.string(),
  reason: z.string(),
});

/** Registry of schemas by event type */
export const EVENT_SCHEMAS: Record<string, z.ZodSchema> = {
  'agent.spawned': AgentSpawnedSchema,
  'agent.stopped': AgentStoppedSchema,
  'agent.crashed': AgentCrashedSchema,
  'tool.called': ToolCalledSchema,
  'tool.denied': ToolDeniedSchema,
  'permission.requested': PermissionRequestedSchema,
  'permission.granted': PermissionDecisionSchema,
  'permission.denied': PermissionDecisionSchema,
  'secret.accessed': SecretAccessedSchema,
  'budget.exceeded': BudgetExceededSchema,
  'schedule.fired': ScheduleFiredSchema,
  'knowledge.committed': KnowledgeCommittedSchema,
  'api-key.created': ApiKeyCreatedSchema,
  'api-key.revoked': ApiKeyRevokedSchema,
  'audit.exported': AuditExportedSchema,
  'circle.created': CircleCreatedSchema,
  'circle.deleted': CircleDeletedSchema,
  'circle.membership_changed': CircleMembershipChangedSchema,
  'delegation.created': DelegationCreatedSchema,
  'delegation.revoked': DelegationRevokedSchema,
  'delegation.derived': DelegationDerivedSchema,
  'credential.resolution.denied': CredentialResolutionDeniedSchema,
};

/**
 * Validates an audit payload against its registered schema.
 * If no schema is registered, it accepts the payload as-is (Story 11.4: arbitrary payloads).
 */
export function validatePayload(eventType: string, payload: unknown): unknown {
  const schema = EVENT_SCHEMAS[eventType];
  if (!schema) return payload;
  return schema.parse(payload);
}
