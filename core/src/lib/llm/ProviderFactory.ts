import { OpenAIProvider } from './OpenAIProvider.js';
import { LlmRouterProvider } from './LlmRouterProvider.js';
import type { LLMProvider } from './types.js';
import type { AgentManifest, ModelConfig } from '../../agents/manifest/types.js';
import { config } from '../config.js';
import type { LlmRouter } from '../../llm/LlmRouter.js';

/**
 * ProviderFactory — creates LLMProvider instances from manifest model config.
 *
 * Resolves the `model` block in an AGENT.yaml to a concrete LLMProvider.
 * When an LlmRouter is supplied the factory returns an LlmRouterProvider backed
 * by the new in-process pi-mono gateway; otherwise it falls back to OpenAIProvider
 * (legacy path).
 */
export class ProviderFactory {
  /**
   * Create an LLMProvider from a manifest's model block.
   *
   * Supports both flat format (top-level `model`) and spec-wrapped format
   * (`spec.model`). Falls back to the default global provider when neither
   * is present or when the model name is absent.
   *
   * When `router` is provided the returned provider uses LlmRouter → pi-mono
   * instead of the legacy OpenAIProvider + litellm chain.
   */
  static createFromManifest(manifest: AgentManifest, router?: LlmRouter): LLMProvider {
    // Prefer flat top-level model; fall back to spec.model for new-format manifests
    const modelConfig: ModelConfig | undefined =
      manifest.model ??
      (manifest.spec?.model?.name
        ? {
            provider: manifest.spec.model.provider ?? '',
            name: manifest.spec.model.name,
            ...(manifest.spec.model.temperature !== undefined
              ? { temperature: manifest.spec.model.temperature }
              : {}),
          }
        : undefined);

    if (!modelConfig?.name) {
      // No model configured — return the default provider
      return ProviderFactory.createDefault(router);
    }

    if (router) {
      return new LlmRouterProvider(
        router,
        modelConfig.name,
        ...(modelConfig.temperature !== undefined ? [modelConfig.temperature] : [])
      );
    }

    return ProviderFactory.createFromModelConfig(modelConfig);
  }

  /**
   * Create an LLMProvider from a ModelConfig.
   */
  static createFromModelConfig(modelConfig: ModelConfig): LLMProvider {
    const providerConfig = config.getProviderConfig(modelConfig.provider);

    const temp =
      modelConfig.temperature !== undefined ? { temperature: modelConfig.temperature } : {};

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
  static createDefault(_router?: LlmRouter): LLMProvider {
    return new OpenAIProvider();
  }
}
