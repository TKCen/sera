import { useQuery } from '@tanstack/react-query';
import * as templatesApi from '@/lib/api/templates';

export function useTemplates() {
  return useQuery({
    queryKey: ['templates'],
    queryFn: templatesApi.listTemplates,
  });
}

export function useTemplate(name: string) {
  return useQuery({
    queryKey: ['templates', name],
    queryFn: () => templatesApi.getTemplate(name),
    enabled: name.length > 0,
  });
}
