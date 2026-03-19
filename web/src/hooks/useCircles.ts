import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import * as circlesApi from '@/lib/api/circles';
import type { CircleManifest } from '@/lib/api/types';

export const circlesKeys = {
  all: ['circles'] as const,
  detail: (name: string) => ['circles', name] as const,
};

export function useCircles() {
  return useQuery({
    queryKey: circlesKeys.all,
    queryFn: circlesApi.listCircles,
  });
}

export function useCircle(name: string) {
  return useQuery({
    queryKey: circlesKeys.detail(name),
    queryFn: () => circlesApi.getCircle(name),
    enabled: name.length > 0,
  });
}

export function useCreateCircle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (manifest: CircleManifest) => circlesApi.createCircle(manifest),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: circlesKeys.all });
    },
  });
}

export function useUpdateCircle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ name, manifest }: { name: string; manifest: CircleManifest }) =>
      circlesApi.updateCircle(name, manifest),
    onSuccess: (_data, { name }) => {
      void qc.invalidateQueries({ queryKey: circlesKeys.detail(name) });
      void qc.invalidateQueries({ queryKey: circlesKeys.all });
    },
  });
}

export function useDeleteCircle() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => circlesApi.deleteCircle(name),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: circlesKeys.all });
    },
  });
}
