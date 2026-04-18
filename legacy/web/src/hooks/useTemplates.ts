import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as templatesApi from '@/lib/api/templates';
import type { AgentTemplate } from '@/lib/api/types';

export const templatesKeys = {
  all: ['templates'] as const,
  detail: (name: string) => ['templates', name] as const,
};

export function useTemplates() {
  return useQuery({
    queryKey: templatesKeys.all,
    queryFn: templatesApi.listTemplates,
  });
}

export function useTemplate(name: string) {
  return useQuery({
    queryKey: templatesKeys.detail(name),
    queryFn: () => templatesApi.getTemplate(name),
    enabled: name.length > 0,
  });
}

export function useCreateTemplate() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (template: AgentTemplate) => templatesApi.createTemplate(template),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: templatesKeys.all });
    },
  });
}

export function useUpdateTemplate() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ name, template }: { name: string; template: AgentTemplate }) =>
      templatesApi.updateTemplate(name, template),
    onSuccess: (_data, { name }) => {
      void qc.invalidateQueries({ queryKey: templatesKeys.all });
      void qc.invalidateQueries({ queryKey: templatesKeys.detail(name) });
    },
  });
}

export function useDeleteTemplate() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => templatesApi.deleteTemplate(name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: templatesKeys.all });
    },
  });
}
