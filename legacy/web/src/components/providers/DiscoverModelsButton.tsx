import { useState } from 'react';
import { Search, Loader2 } from 'lucide-react';
import { toast } from 'sonner';
import { useDiscoverModels, useAddProvider } from '@/hooks/useProviders';
import { Button } from '@/components/ui/button';

export function DiscoverModelsButton({ providerName }: { providerName: string }) {
  const discover = useDiscoverModels();
  const addProvider = useAddProvider();
  const [models, setModels] = useState<string[] | null>(null);

  const handleDiscover = async () => {
    try {
      const result = await discover.mutateAsync(providerName);
      const discovered = result.models ?? [];
      setModels(discovered);
      toast.success(`Found ${discovered.length} models`);
    } catch {
      toast.error('Discovery failed');
    }
  };

  const handleAddModel = async (modelName: string) => {
    try {
      await addProvider.mutateAsync({
        modelName,
        api: 'openai',
        provider: providerName,
      });
      toast.success(`Added ${modelName}`);
      setModels((prev) => prev?.filter((m) => m !== modelName) ?? null);
    } catch {
      toast.error(`Failed to add ${modelName}`);
    }
  };

  return (
    <div>
      <Button
        size="sm"
        variant="ghost"
        onClick={() => void handleDiscover()}
        disabled={discover.isPending}
      >
        {discover.isPending ? <Loader2 size={12} className="animate-spin" /> : <Search size={12} />}
        Discover
      </Button>
      {models && models.length > 0 && (
        <div className="bg-sera-surface-active/40 rounded-lg p-2.5 mt-3 ml-4 border border-sera-border/50 space-y-1">
          {models.map((m) => (
            <div key={m} className="flex items-center gap-2 text-xs">
              <span className="font-mono text-sera-text-muted">{m}</span>
              <button
                type="button"
                onClick={() => void handleAddModel(m)}
                className="text-sera-accent hover:underline text-[10px]"
              >
                + Add
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
