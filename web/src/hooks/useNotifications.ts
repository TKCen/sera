import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { toast } from 'sonner';
import {
  listChannels,
  createChannel,
  deleteChannel,
  testChannel,
  listRoutingRules,
  createRoutingRule,
  deleteRoutingRule,
  type CreateChannelPayload,
  type CreateRoutingRulePayload,
} from '@/lib/api/notifications';

const CHANNELS_KEY = ['notification-channels'] as const;
const RULES_KEY = ['notification-routing-rules'] as const;

export function useChannels() {
  return useQuery({ queryKey: CHANNELS_KEY, queryFn: listChannels });
}

export function useRoutingRules() {
  return useQuery({ queryKey: RULES_KEY, queryFn: listRoutingRules });
}

export function useCreateChannel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (data: CreateChannelPayload) => createChannel(data),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: CHANNELS_KEY });
      toast.success('Channel created');
    },
    onError: (err: Error) => toast.error(`Failed to create channel: ${err.message}`),
  });
}

export function useDeleteChannel() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteChannel(id),
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
    mutationFn: (id: string) => testChannel(id),
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
    mutationFn: (data: CreateRoutingRulePayload) => createRoutingRule(data),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: RULES_KEY });
      toast.success('Routing rule created');
    },
    onError: (err: Error) => toast.error(`Failed to create routing rule: ${err.message}`),
  });
}

export function useDeleteRoutingRule() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteRoutingRule(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: RULES_KEY });
      toast.success('Routing rule deleted');
    },
    onError: (err: Error) => toast.error(`Failed to delete rule: ${err.message}`),
  });
}
