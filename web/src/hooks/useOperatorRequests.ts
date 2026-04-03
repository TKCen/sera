import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { listOperatorRequests, respondToRequest } from '@/lib/api/operator-requests';

export function useOperatorRequests(status?: string) {
  return useQuery({
    queryKey: ['operator-requests', status],
    queryFn: () => listOperatorRequests(status),
    refetchInterval: 5000,
  });
}

export function useRespondToRequest() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      action,
      response,
    }: {
      id: string;
      action: 'approved' | 'rejected' | 'resolved';
      response?: string;
    }) => respondToRequest(id, action, response),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['operator-requests'] });
    },
  });
}
