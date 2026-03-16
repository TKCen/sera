import { OpenAIProvider } from './OpenAIProvider.js';
import type { LLMProvider } from './types.js';
import type { AgentManifest, ModelConfig } from '../../agents/manifest/types.js';
import { config } from '../config.js';

/**
 * ProviderFactory — creates LLMProvider instances from manifest model config.
 *
 * Resolves the `model` block in an AGENT.yaml to a concrete LLMProvider.
 * Currently all providers use the OpenAI-compatible protocol (which covers
 * LM Studio, Ollama, OpenAI, Anthropic via proxy, etc.).
 */
export class ProviderFactory {
  /**
   * Create an LLMProvider from a manifest's model block.
   *
   * Looks up the provider ID in the saved providers.json config to resolve
   * the baseUrl and apiKey. The model name and temperature come from the manifest.
   */
  static createFromManifest(manifest: AgentManifest): LLMProvider {
    return ProviderFactory.createFromModelConfig(manifest.model);
  }

  /**
   * Create an LLMProvider from a ModelConfig.
   */
  static createFromModelConfig(modelConfig: ModelConfig): LLMProvider {
    const providerConfig = config.getProviderConfig(modelConfig.provider);

    const temp = modelConfig.temperature !== undefined
      ? { temperature: modelConfig.temperature }
      : {};

    if (providerConfig?.baseUrl) {
      return new OpenAIProvider({
        baseUrl: providerConfig.baseUrl,
        apiKey: providerConfig.apiKey || 'not-needed',
        model: modelConfig.name,
        ...temp,
      });
    }

    // Fallback: use the global active provider config with the manifest's model name
    const globalConfig = config.llm;
    return new OpenAIProvider({
      baseUrl: globalConfig.baseUrl,
      apiKey: globalConfig.apiKey,
      model: modelConfig.name,
      ...temp,
    });
  }

  /**
   * Create the default provider from the global config (no manifest).
   */
  static createDefault(): LLMProvider {
    return new OpenAIProvider();
  }
}
