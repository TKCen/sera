import { useQuery, useMutation } from '@tanstack/react-query';
import * as auditApi from '@/lib/api/audit';
import type { AuditParams } from '@/lib/api/audit';

export const auditKeys = {
  events: (params: AuditParams) => ['audit', 'events', params] as const,
  verify: ['audit', 'verify'] as const,
};

export function useAuditEvents(params: AuditParams) {
  return useQuery({
    queryKey: auditKeys.events(params),
    queryFn: () => auditApi.getAuditEvents(params),
    placeholderData: (prev) => prev,
  });
}

export function useVerifyAuditChain() {
  return useMutation({
    mutationFn: () => auditApi.verifyAuditChain(),
  });
}
