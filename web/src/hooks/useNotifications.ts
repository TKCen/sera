import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import * as notificationsApi from '@/lib/api/notifications';
import {
  type CreateChannelPayload,
  type CreateRoutingRulePayload,
  type NotificationChannel,
  type RoutingRule,
} from '@/lib/api/notifications';

export type { NotificationChannel, RoutingRule, CreateChannelPayload, CreateRoutingRulePayload };

const CHANNELS_KEY = ['notification-channels'] as const;
const RULES_KEY = ['notification-routing-rules'] as const;

export function useChannels() {
  return useQuery({ queryKey: CHANNELS_KEY, queryFn: notificationsApi.listChannels });
}

export function useRoutingRules() {
  return useQuery({ queryKey: RULES_KEY, queryFn: notificationsApi.listRoutingRules });
}

export function useCreateChannel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateChannelPayload) => notificationsApi.createChannel(data),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: CHANNELS_KEY });
      toast.success('Channel created');
    },
    onError: (err: Error) => toast.error(`Failed to create channel: ${err.message}`),
  });
}

export function useUpdateChannel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: Partial<CreateChannelPayload> }) =>
      notificationsApi.updateChannel(id, data),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: CHANNELS_KEY });
      toast.success('Channel updated');
    },
    onError: (err: Error) => toast.error(`Failed to update channel: ${err.message}`),
  });
}

export function useDeleteChannel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => notificationsApi.deleteChannel(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: CHANNELS_KEY });
      void qc.invalidateQueries({ queryKey: RULES_KEY });
      toast.success('Channel deleted');
    },
    onError: (err: Error) => toast.error(`Failed to delete channel: ${err.message}`),
  });
}

export function useTestChannel() {
  return useMutation({
    mutationFn: (id: string) => notificationsApi.testChannel(id),
    onSuccess: (result) => {
      if (result.ok) {
        toast.success('Test notification delivered');
      } else {
        toast.error(`Delivery failed: ${result.error ?? 'unknown error'}`);
      }
    },
    onError: (err: Error) => toast.error(`Test failed: ${err.message}`),
  });
}

export function useCreateRoutingRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateRoutingRulePayload) => notificationsApi.createRoutingRule(data),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: RULES_KEY });
      toast.success('Routing rule created');
    },
    onError: (err: Error) => toast.error(`Failed to create routing rule: ${err.message}`),
  });
}

export function useUpdateRoutingRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ id, data }: { id: string; data: Partial<CreateRoutingRulePayload> }) =>
      notificationsApi.updateRoutingRule(id, data),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: RULES_KEY });
      toast.success('Routing rule updated');
    },
    onError: (err: Error) => toast.error(`Failed to update routing rule: ${err.message}`),
  });
}

export function useDeleteRoutingRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => notificationsApi.deleteRoutingRule(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: RULES_KEY });
      toast.success('Routing rule deleted');
    },
    onError: (err: Error) => toast.error(`Failed to delete rule: ${err.message}`),
  });
}
