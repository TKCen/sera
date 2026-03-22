import { useState, useEffect } from 'react';
import { toast } from 'sonner';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { useSearchRegistry, useImportSkill } from '@/hooks/useSkills';
import { Search, Download, Globe, Star } from 'lucide-react';
import { cn } from '@/lib/utils';

interface SkillImportDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function SkillImportDialog({ open, onOpenChange }: SkillImportDialogProps) {
  const [query, setQuery] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const importSkill = useImportSkill();

  const { data: results, isLoading } = useSearchRegistry(
    debouncedQuery || ' ', // Empty query browses trending
    'clawhub'
  );

  // Debounce search
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedQuery(query), 400);
    return () => clearTimeout(timer);
  }, [query]);

  // Reset on close
  useEffect(() => {
    if (!open) {
      setQuery('');
      setDebouncedQuery('');
    }
  }, [open]);

  async function handleImport(skillId: string, skillName: string) {
    try {
      await importSkill.mutateAsync({ source: 'clawhub', skillId });
      toast.success(`Skill "${skillName}" imported from ClawHub`);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Import failed');
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-hidden flex flex-col">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Globe size={16} className="text-sera-accent" />
            Import from ClawHub
          </DialogTitle>
          <DialogDescription>
            Browse and import community skills from{' '}
            <span className="text-sera-accent">clawhub.ai</span> — 3,000+ skills available.
          </DialogDescription>
        </DialogHeader>

        {/* Search */}
        <div className="relative">
          <Search
            size={13}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-sera-text-dim"
          />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search ClawHub skills…"
            className="pl-8"
          />
        </div>

        {/* Results */}
        <div className="flex-1 overflow-y-auto space-y-1 min-h-0">
          {isLoading ? (
            <div className="py-8 text-center text-xs text-sera-text-dim">Searching ClawHub…</div>
          ) : !results || results.length === 0 ? (
            <div className="py-8 text-center text-xs text-sera-text-dim">
              {debouncedQuery ? 'No skills found' : 'Type to search or browse trending skills'}
            </div>
          ) : (
            results.map((skill) => (
              <div
                key={skill.id}
                className={cn(
                  'flex items-start justify-between gap-3 p-3 rounded-lg',
                  'border border-sera-border/50 hover:border-sera-border transition-colors'
                )}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 flex-wrap">
                    <span className="font-medium text-sm text-sera-text">{skill.name}</span>
                    {skill.version && <Badge variant="default">v{skill.version}</Badge>}
                    <Badge variant="accent">clawhub</Badge>
                  </div>
                  <p className="text-xs text-sera-text-muted mt-0.5 line-clamp-2">
                    {skill.description}
                  </p>
                  {skill.tags && skill.tags.length > 0 && (
                    <div className="flex items-center gap-1 mt-1 flex-wrap">
                      {skill.tags.slice(0, 5).map((tag) => (
                        <span
                          key={tag}
                          className="text-[9px] px-1 py-0.5 rounded bg-sera-surface-hover text-sera-text-dim"
                        >
                          {tag}
                        </span>
                      ))}
                    </div>
                  )}
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    void handleImport(skill.id, skill.name);
                  }}
                  disabled={importSkill.isPending}
                  className="flex-shrink-0"
                >
                  <Download size={12} />
                  Import
                </Button>
              </div>
            ))
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between pt-2 border-t border-sera-border">
          <a
            href="https://clawhub.ai"
            target="_blank"
            rel="noopener noreferrer"
            className="text-[10px] text-sera-text-dim hover:text-sera-accent transition-colors flex items-center gap-1"
          >
            <Star size={9} /> Browse all skills on clawhub.ai
          </a>
          <Button variant="ghost" size="sm" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
