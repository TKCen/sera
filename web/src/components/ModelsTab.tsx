import { ModelConfigRow } from './ModelConfigRow';

interface Model {
  modelName: string;
  provider?: string;
  api: string;
  baseUrl?: string;
  description?: string;
}

export function ModelsTab({
  registeredModels,
  refetchProviders,
}: {
  registeredModels: Model[];
  refetchProviders: () => void;
}) {
  return (
    <div className="sera-card-static p-5">
      {registeredModels.length === 0 ? (
        <div className="text-center py-12 text-sera-text-dim text-sm">
          No models registered. Add a provider in the Providers tab.
        </div>
      ) : (
        <div className="space-y-2">
          {registeredModels.map((m) => {
            const ext = m as unknown as Record<string, unknown>;
            return (
              <ModelConfigRow
                key={m.modelName}
                model={m}
                authStatus={ext.authStatus as string}
                contextWindow={ext.contextWindow as number | undefined}
                maxTokens={ext.maxTokens as number | undefined}
                contextStrategy={ext.contextStrategy as string | undefined}
                contextHighWaterMark={ext.contextHighWaterMark as number | undefined}
                reasoning={ext.reasoning as boolean | undefined}
                onSaved={() => void refetchProviders()}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}
