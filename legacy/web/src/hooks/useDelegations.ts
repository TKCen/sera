import { useMutation, useQueryClient } from '@tanstack/react-query';
import * as delegationApi from '@/lib/api/delegation';
import { agentsKeys } from './useAgents';

export function useIssueDelegation() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (params: delegationApi.IssueDelegationParams) =>
      delegationApi.issueDelegation(params),
    onSuccess: (_data, { agentId }) => {
      void qc.invalidateQueries({ queryKey: agentsKeys.delegations(agentId) });
    },
  });
}
