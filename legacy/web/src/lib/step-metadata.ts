/**
 * Shared step type metadata for thought/reasoning visualization components.
 *
 * Used by ThoughtTimeline, ChatThoughtPanel, and CommandLogTimeline to maintain
 * consistent step type → label/color mappings across the UI.
 */

export type StepType =
  | 'observe'
  | 'plan'
  | 'act'
  | 'reflect'
  | 'tool-call'
  | 'tool-result'
  | 'reasoning'
  | 'error';

export interface StepMeta {
  label: string;
  /** Icon name from lucide-react */
  icon: string;
  /** Tailwind text-color class using sera theme tokens */
  color: string;
  /** Tailwind bg + border classes for card-style rendering */
  bg: string;
}

export const STEP_META: Record<StepType, StepMeta> = {
  observe: {
    label: 'Observe',
    icon: 'Eye',
    color: 'text-sera-info',
    bg: 'bg-sera-info/15 border-sera-info/30',
  },
  plan: {
    label: 'Plan',
    icon: 'Map',
    color: 'text-sera-warning',
    bg: 'bg-sera-warning/15 border-sera-warning/30',
  },
  act: {
    label: 'Act',
    icon: 'Zap',
    color: 'text-sera-success',
    bg: 'bg-sera-success/15 border-sera-success/30',
  },
  reflect: {
    label: 'Reflect',
    icon: 'RotateCcw',
    color: 'text-purple-400',
    bg: 'bg-purple-400/15 border-purple-400/30',
  },
  'tool-call': {
    label: 'Tool',
    icon: 'Wrench',
    color: 'text-sera-warning',
    bg: 'bg-sera-warning/15 border-sera-warning/30',
  },
  'tool-result': {
    label: 'Result',
    icon: 'CheckCircle2',
    color: 'text-sera-success',
    bg: 'bg-sera-success/10 border-sera-success/20',
  },
  reasoning: {
    label: 'Reason',
    icon: 'Brain',
    color: 'text-sera-text-muted',
    bg: 'bg-sera-surface-hover border-sera-border',
  },
  error: {
    label: 'Error',
    icon: 'AlertTriangle',
    color: 'text-sera-error',
    bg: 'bg-sera-error/10 border-sera-error/30',
  },
};

/** Get step metadata with fallback to reasoning for unknown types */
export function getStepMeta(stepType: string): StepMeta {
  return STEP_META[stepType as StepType] ?? STEP_META.reasoning;
}
