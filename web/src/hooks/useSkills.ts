import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as skillsApi from '@/lib/api/skills';
import type { CreateSkillParams } from '@/lib/api/types';

export const skillsKeys = {
  all: ['skills'] as const,
  registry: (query: string, source?: string) => ['skills', 'registry', query, source] as const,
};

export function useSkills() {
  return useQuery({
    queryKey: skillsKeys.all,
    queryFn: skillsApi.listSkills,
  });
}

export function useCreateSkill() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (params: CreateSkillParams) => skillsApi.createSkill(params),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: skillsKeys.all });
    },
  });
}

export function useDeleteSkill() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => skillsApi.deleteSkill(name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: skillsKeys.all });
    },
  });
}

export function useSearchRegistry(query: string, source?: string) {
  return useQuery({
    queryKey: skillsKeys.registry(query, source),
    queryFn: () => skillsApi.searchRegistry(query, source),
    enabled: query.length > 0,
  });
}

export function useImportSkill() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ source, skillId }: { source: string; skillId: string }) =>
      skillsApi.importSkill(source, skillId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: skillsKeys.all });
    },
  });
}
