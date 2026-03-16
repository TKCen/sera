import fs from 'fs';
import path from 'path';

const CONFIG_PATH = path.join(process.cwd(), 'config', 'llm.json');

export interface LLMConfig {
  provider: 'lm-studio' | 'openai' | string;
  baseUrl: string;
  apiKey: string;
  model: string;
}

const defaultConfig: LLMConfig = {
  provider: process.env.LLM_PROVIDER || 'lm-studio',
  baseUrl: process.env.LM_STUDIO_URL || process.env.LLM_BASE_URL || 'http://localhost:1234/v1',
  apiKey: process.env.LLM_API_KEY || 'lm-studio',
  model: process.env.LM_STUDIO_MODEL || process.env.LLM_MODEL || 'meta-llama-3-8b-instruct',
};

function loadConfig(): LLMConfig {
  try {
    if (fs.existsSync(CONFIG_PATH)) {
      const data = fs.readFileSync(CONFIG_PATH, 'utf-8');
      return { ...defaultConfig, ...JSON.parse(data) };
    }
  } catch (err) {
    console.error('Failed to load LLM config:', err);
  }
  return defaultConfig;
}

export const config = {
  get llm(): LLMConfig {
    return loadConfig();
  },
  saveLlmConfig(newConfig: LLMConfig) {
    try {
      const dir = path.dirname(CONFIG_PATH);
      if (!fs.existsSync(dir)) {
        fs.mkdirSync(dir, { recursive: true });
      }
      fs.writeFileSync(CONFIG_PATH, JSON.stringify(newConfig, null, 2));
    } catch (err) {
      console.error('Failed to save LLM config:', err);
      throw err;
    }
  },
  databaseUrl: process.env.DATABASE_URL,
  port: process.env.PORT || 3001,
};
