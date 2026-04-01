import { useState, useMemo } from 'react';
import { Link, useNavigate } from 'react-router';
import { Users, Plus, Search, Radio, Bot, FileText } from 'lucide-react';
import { toast } from 'sonner';
import { useCircles, useCreateCircle } from '@/hooks/useCircles';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';

export default function CirclesPage() {
  const { data: circles, isLoading } = useCircles();
  const createCircle = useCreateCircle();
  const navigate = useNavigate();

  const [search, setSearch] = useState('');
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState('');
  const [newDisplayName, setNewDisplayName] = useState('');
  const [newDescription, setNewDescription] = useState('');

  const filtered = useMemo(() => {
    if (!circles) return [];
    if (!search) return circles;
    const q = search.toLowerCase();
    return circles.filter(
      (c) =>
        c.name.toLowerCase().includes(q) ||
        c.displayName.toLowerCase().includes(q) ||
        (c.description ?? '').toLowerCase().includes(q)
    );
  }, [circles, search]);

  async function handleCreate() {
    if (!newName.trim()) {
      toast.error('Name is required');
      return;
    }
    try {
      await createCircle.mutateAsync({
        apiVersion: 'sera/v1',
        kind: 'Circle',
        metadata: {
          name: newName.trim(),
          displayName: newDisplayName.trim() || newName.trim(),
          ...(newDescription.trim() ? { description: newDescription.trim() } : {}),
        },
        agents: [],
      });
      toast.success(`Created circle "${newDisplayName.trim() || newName.trim()}"`);
      setShowCreate(false);
      setNewName('');
      setNewDisplayName('');
      setNewDescription('');
      void navigate(`/circles/${newName.trim()}`);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to create circle');
    }
  }

  return (
    <div className="p-6">
      <div className="sera-page-header">
        <h1 className="sera-page-title">Circles</h1>
        <Button size="sm" onClick={() => setShowCreate(true)}>
          <Plus size={14} />
          New Circle
        </Button>
      </div>

      {/* Search */}
      {(circles?.length ?? 0) > 0 && (
        <div className="flex items-center gap-3 mb-5">
          <div className="relative flex-1 max-w-xs">
            <Search
              size={13}
              className="absolute left-2.5 top-1/2 -translate-y-1/2 text-sera-text-dim pointer-events-none"
            />
            <Input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search circles…"
              className="pl-8"
            />
          </div>
        </div>
      )}

      {isLoading ? (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-52 rounded-xl" />
          ))}
        </div>
      ) : !circles?.length ? (
        <EmptyState
          icon={<Users size={24} />}
          title="No circles"
          description="Circles group agents into collaborative teams with shared knowledge and channels."
          action={
            <Button size="sm" onClick={() => setShowCreate(true)}>
              <Plus size={14} />
              Create Circle
            </Button>
          }
        />
      ) : filtered.length === 0 ? (
        <p className="text-sm text-sera-text-muted text-center py-12">
          No circles match your search.
        </p>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {filtered.map((circle) => (
            <div
              key={circle.name}
              className="sera-card relative rounded-xl p-5 group flex flex-col gap-4"
            >
              {/* Header row */}
              <div className="flex items-start gap-3">
                <div className="h-10 w-10 rounded-lg bg-sera-accent-soft flex items-center justify-center flex-shrink-0">
                  <Users size={18} className="text-sera-accent" />
                </div>
                <div className="min-w-0 flex-1">
                  <h3 className="font-semibold text-sm text-sera-text truncate">
                    {circle.displayName}
                  </h3>
                  <span className="text-xs text-sera-text-dim">{circle.name}</span>
                </div>
              </div>

              {/* Description */}
              {circle.description && (
                <p className="text-xs text-sera-text-muted line-clamp-2 leading-relaxed">
                  {circle.description}
                </p>
              )}

              {/* Agent membership ring */}
              {(circle.agents?.length ?? 0) > 0 && (
                <div className="flex items-center gap-2">
                  <div className="flex -space-x-2">
                    {circle.agents!.slice(0, 5).map((agent) => (
                      <div
                        key={agent}
                        className="h-7 w-7 rounded-full bg-sera-surface-active border-2 border-sera-bg flex items-center justify-center"
                        title={agent}
                      >
                        <Bot size={12} className="text-sera-text-muted" />
                      </div>
                    ))}
                    {circle.agents!.length > 5 && (
                      <div className="h-7 w-7 rounded-full bg-sera-surface-active border-2 border-sera-bg flex items-center justify-center">
                        <span className="text-[10px] font-medium text-sera-text-muted">
                          +{circle.agents!.length - 5}
                        </span>
                      </div>
                    )}
                  </div>
                  <span className="text-xs text-sera-text-dim">
                    {circle.agents!.length} agent{circle.agents!.length !== 1 ? 's' : ''}
                  </span>
                </div>
              )}

              {/* Stats row */}
              <div className="flex items-center gap-3 flex-wrap mt-auto">
                {(circle.channelCount ?? 0) > 0 && (
                  <Badge variant="default" className="gap-1">
                    <Radio size={10} />
                    {circle.channelCount} channel{circle.channelCount !== 1 ? 's' : ''}
                  </Badge>
                )}
                {circle.hasProjectContext && (
                  <Badge variant="info" className="gap-1">
                    <FileText size={10} />
                    Context
                  </Badge>
                )}
              </div>

              {/* Overlay link */}
              <Link
                to={`/circles/${circle.name}`}
                className="absolute inset-0 rounded-xl"
                aria-label={`View ${circle.displayName}`}
              />
            </div>
          ))}
        </div>
      )}

      {/* Create dialog */}
      <Dialog open={showCreate} onOpenChange={setShowCreate}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create Circle</DialogTitle>
            <DialogDescription>
              A circle groups agents into a collaborative team with shared channels and knowledge.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div>
              <label className="text-xs font-medium text-sera-text-muted mb-1 block">
                Name (identifier)
              </label>
              <Input
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                placeholder="my-circle"
              />
            </div>
            <div>
              <label className="text-xs font-medium text-sera-text-muted mb-1 block">
                Display Name
              </label>
              <Input
                value={newDisplayName}
                onChange={(e) => setNewDisplayName(e.target.value)}
                placeholder="My Circle"
              />
            </div>
            <div>
              <label className="text-xs font-medium text-sera-text-muted mb-1 block">
                Description
              </label>
              <Input
                value={newDescription}
                onChange={(e) => setNewDescription(e.target.value)}
                placeholder="What does this circle do?"
              />
            </div>
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="ghost" size="sm" onClick={() => setShowCreate(false)}>
                Cancel
              </Button>
              <Button
                size="sm"
                onClick={() => {
                  void handleCreate();
                }}
                disabled={createCircle.isPending || !newName.trim()}
              >
                {createCircle.isPending ? 'Creating…' : 'Create'}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
