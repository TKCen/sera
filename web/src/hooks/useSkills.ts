import { useQuery } from '@tanstack/react-query';
import * as skillsApi from '@/lib/api/skills';

export const skillsKeys = {
  all: ['skills'] as const,
};

export function useSkills() {
  return useQuery({
    queryKey: skillsKeys.all,
    queryFn: skillsApi.listSkills,
  });
}
