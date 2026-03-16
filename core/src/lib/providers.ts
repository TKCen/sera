export type ProviderCategory = 'local' | 'cloud';

export interface ProviderDefinition {
  id: string;
  name: string;
  category: ProviderCategory;
  defaultBaseUrl: string;
  requiresKey: boolean;
  description: string;
  models: ProviderModel[];
}

export interface ProviderModel {
  id: string;
  name: string;
  tier: 'frontier' | 'smart' | 'balanced' | 'fast' | 'local';
  contextWindow: number;
}

export interface ProviderConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
}

export interface ProvidersConfig {
  activeProvider: string;
  providers: Record<string, ProviderConfig>;
}

/** Curated starter catalog — focused on homelab-relevant providers */
export const PROVIDER_CATALOG: ProviderDefinition[] = [
  // ─── Local Providers ────────────────────────────────────────────────
  {
    id: 'lmstudio',
    name: 'LM Studio',
    category: 'local',
    defaultBaseUrl: 'http://host.docker.internal:1234/v1',
    requiresKey: false,
    description: 'Run open-source models locally with a desktop app. OpenAI-compatible API.',
    models: [
      { id: 'lmstudio-local', name: 'Local Model', tier: 'local', contextWindow: 32768 },
    ],
  },
  {
    id: 'ollama',
    name: 'Ollama',
    category: 'local',
    defaultBaseUrl: 'http://host.docker.internal:11434/v1',
    requiresKey: false,
    description: 'Self-hosted model runner. Pull and serve any open model with one command.',
    models: [
      { id: 'llama3.2', name: 'Llama 3.2', tier: 'local', contextWindow: 128000 },
      { id: 'mistral:latest', name: 'Mistral', tier: 'local', contextWindow: 32768 },
      { id: 'phi3', name: 'Phi-3', tier: 'local', contextWindow: 128000 },
    ],
  },

  // ─── Cloud Providers ────────────────────────────────────────────────
  {
    id: 'openai',
    name: 'OpenAI',
    category: 'cloud',
    defaultBaseUrl: 'https://api.openai.com/v1',
    requiresKey: true,
    description: 'GPT-4o, GPT-4.1, o3 and more. Industry-standard API.',
    models: [
      { id: 'gpt-4.1', name: 'GPT-4.1', tier: 'frontier', contextWindow: 1047576 },
      { id: 'gpt-4o', name: 'GPT-4o', tier: 'smart', contextWindow: 128000 },
      { id: 'gpt-4o-mini', name: 'GPT-4o Mini', tier: 'fast', contextWindow: 128000 },
      { id: 'gpt-4.1-mini', name: 'GPT-4.1 Mini', tier: 'balanced', contextWindow: 1047576 },
      { id: 'o3-mini', name: 'o3-mini', tier: 'smart', contextWindow: 200000 },
    ],
  },
  {
    id: 'anthropic',
    name: 'Anthropic',
    category: 'cloud',
    defaultBaseUrl: 'https://api.anthropic.com',
    requiresKey: true,
    description: 'Claude Opus, Sonnet, and Haiku. Best-in-class reasoning.',
    models: [
      { id: 'claude-opus-4-20250514', name: 'Claude Opus 4', tier: 'frontier', contextWindow: 200000 },
      { id: 'claude-sonnet-4-20250514', name: 'Claude Sonnet 4', tier: 'smart', contextWindow: 200000 },
      { id: 'claude-haiku-4-5-20251001', name: 'Claude Haiku 4.5', tier: 'fast', contextWindow: 200000 },
    ],
  },
  {
    id: 'gemini',
    name: 'Google Gemini',
    category: 'cloud',
    defaultBaseUrl: 'https://generativelanguage.googleapis.com',
    requiresKey: true,
    description: 'Gemini 2.5 Pro and Flash. Huge context windows, generous free tier.',
    models: [
      { id: 'gemini-2.5-pro', name: 'Gemini 2.5 Pro', tier: 'frontier', contextWindow: 1048576 },
      { id: 'gemini-2.5-flash', name: 'Gemini 2.5 Flash', tier: 'smart', contextWindow: 1048576 },
      { id: 'gemini-2.0-flash', name: 'Gemini 2.0 Flash', tier: 'fast', contextWindow: 1048576 },
    ],
  },
  {
    id: 'deepseek',
    name: 'DeepSeek',
    category: 'cloud',
    defaultBaseUrl: 'https://api.deepseek.com/v1',
    requiresKey: true,
    description: 'DeepSeek V3 and R1. Strong reasoning at extremely low cost.',
    models: [
      { id: 'deepseek-chat', name: 'DeepSeek V3', tier: 'smart', contextWindow: 64000 },
      { id: 'deepseek-reasoner', name: 'DeepSeek R1', tier: 'smart', contextWindow: 64000 },
    ],
  },
  {
    id: 'groq',
    name: 'Groq',
    category: 'cloud',
    defaultBaseUrl: 'https://api.groq.com/openai/v1',
    requiresKey: true,
    description: 'Ultra-fast LPU inference. Free tier with rate limits.',
    models: [
      { id: 'llama-3.3-70b-versatile', name: 'Llama 3.3 70B', tier: 'balanced', contextWindow: 128000 },
      { id: 'mixtral-8x7b-32768', name: 'Mixtral 8x7B', tier: 'balanced', contextWindow: 32768 },
      { id: 'llama-3.1-8b-instant', name: 'Llama 3.1 8B', tier: 'fast', contextWindow: 128000 },
    ],
  },
  {
    id: 'openrouter',
    name: 'OpenRouter',
    category: 'cloud',
    defaultBaseUrl: 'https://openrouter.ai/api/v1',
    requiresKey: true,
    description: 'Unified gateway to 200+ models. One key, all providers.',
    models: [
      { id: 'openrouter/google/gemini-2.5-flash', name: 'Gemini 2.5 Flash', tier: 'smart', contextWindow: 1048576 },
      { id: 'openrouter/anthropic/claude-sonnet-4', name: 'Claude Sonnet 4', tier: 'smart', contextWindow: 200000 },
      { id: 'openrouter/openai/gpt-4o', name: 'GPT-4o', tier: 'smart', contextWindow: 128000 },
      { id: 'openrouter/deepseek/deepseek-chat', name: 'DeepSeek V3', tier: 'smart', contextWindow: 128000 },
    ],
  },
];
