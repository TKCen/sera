import { useQuery } from '@tanstack/react-query';
import { listTools } from '@/lib/api/tools';

export function useTools() {
  return useQuery({
    queryKey: ['tools'],
    queryFn: listTools,
  });
}
