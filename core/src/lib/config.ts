import fs from 'fs';
import path from 'path';
import type { ProviderConfig, ProvidersConfig } from './providers.js';
import { Logger } from './logger.js';

const logger = new Logger('Config');

const CONFIG_PATH = path.join(process.cwd(), 'config', 'llm.json');
const PROVIDERS_CONFIG_PATH = path.join(process.cwd(), 'config', 'providers.json');

// ─── Legacy single-provider config (backward compat) ──────────────────────────
interface LLMConfig {
  baseUrl: string;
  apiKey: string;
  model: string;
}

const defaultConfig: LLMConfig = {
  baseUrl: process.env.LLM_BASE_URL || 'http://localhost:1234/v1',
  apiKey: process.env.LLM_API_KEY || 'lm-studio',
  model: process.env.LLM_MODEL || 'model-identifier',
};

function loadLegacyConfig(): LLMConfig {
  try {
    if (fs.existsSync(CONFIG_PATH)) {
      const data = fs.readFileSync(CONFIG_PATH, 'utf-8');
      return { ...defaultConfig, ...JSON.parse(data) };
    }
  } catch (err) {
    logger.error('Failed to load LLM config:', err);
  }
  return defaultConfig;
}

// ─── Multi-provider config ────────────────────────────────────────────────────
const defaultProvidersConfig: ProvidersConfig = {
  activeProvider: 'lmstudio',
  providers: {},
};

function loadProvidersConfig(): ProvidersConfig {
  try {
    if (fs.existsSync(PROVIDERS_CONFIG_PATH)) {
      const data = fs.readFileSync(PROVIDERS_CONFIG_PATH, 'utf-8');
      return { ...defaultProvidersConfig, ...JSON.parse(data) };
    }
  } catch (err) {
    logger.error('Failed to load providers config:', err);
  }
  return defaultProvidersConfig;
}


export const config = {
  // Legacy single-provider access (used by OpenAIProvider)
  get llm(): LLMConfig {
    // If new multi-provider config exists, derive from the active provider
    const providersConfig = loadProvidersConfig();
    const activeId = providersConfig.activeProvider;
    const activeConf = providersConfig.providers[activeId];

    if (activeConf && activeConf.baseUrl) {
      return {
        baseUrl: activeConf.baseUrl,
        apiKey: activeConf.apiKey || 'not-needed',
        model: activeConf.model || 'model-identifier',
      };
    }
    return loadLegacyConfig();
  },

  saveLlmConfig(newConfig: LLMConfig) {
    try {
      const dir = path.dirname(CONFIG_PATH);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }
      fs.writeFileSync(CONFIG_PATH, JSON.stringify(newConfig, null, 2));
    } catch (err) {
      logger.error('Failed to save LLM config:', err);
      throw err;
    }
  },

  // Multi-provider config
  get providers(): ProvidersConfig {
    return loadProvidersConfig();
  },

  getProviderConfig(providerId: string): ProviderConfig | undefined {
    const cfg = loadProvidersConfig();
    return cfg.providers[providerId];
  },

  channels: {
    telegram: {
      token: process.env.TELEGRAM_BOT_TOKEN,
    },
    discord: {
      token: process.env.DISCORD_BOT_TOKEN,
    },
    whatsapp: {
      token: process.env.WHATSAPP_API_TOKEN,
      phoneNumberId: process.env.WHATSAPP_PHONE_NUMBER_ID,
    },
    rateLimit: {
      windowMs: parseInt(process.env.CHANNEL_RATE_LIMIT_WINDOW_MS || '60000', 10),
      maxMessages: parseInt(process.env.CHANNEL_RATE_LIMIT_MAX_MESSAGES || '20', 10),
    },
  },

  databaseUrl: process.env.DATABASE_URL,
  port: process.env.PORT || 3001,
};
