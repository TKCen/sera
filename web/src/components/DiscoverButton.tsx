import { useState } from 'react';
import { Loader2, Search } from 'lucide-react';
import { toast } from 'sonner';
import { useDiscoverModels, useAddProvider } from '@/hooks/useProviders';
import { Button } from '@/components/ui/button';

export function DiscoverButton({ providerName }: { providerName: string }) {
  const discover = useDiscoverModels();
  const addProvider = useAddProvider();
  const [models, setModels] = useState<string[] | null>(null);

  const handleDiscover = async () => {
    try {
      const result = await discover.mutateAsync(providerName);
      setModels(result.models);
      toast.success(`Found ${result.models.length} models`);
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
        <div className="mt-2 ml-4 space-y-1">
          {models.map((m) => (
            <div key={m} className="flex items-center gap-2 text-xs">
              <span className="font-mono text-sera-text-muted">{m}</span>
              <button
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
