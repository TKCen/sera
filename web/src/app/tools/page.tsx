import { useState, useMemo } from 'react';
import { toast } from 'sonner';
import { Wrench, BookOpen, Search, Cpu, Server, Plus, Download } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import { useTools } from '@/hooks/useTools';
import { useSkills, useDeleteSkill } from '@/hooks/useSkills';
import type { ToolInfo, GuidanceSkillInfo } from '@/lib/api/types';
import { cn } from '@/lib/utils';
import { ToolCard } from '@/components/ToolCard';
import { SkillCard } from '@/components/SkillCard';
import { SkillEditorDialog } from '@/components/SkillEditorDialog';
import { SkillImportDialog } from '@/components/SkillImportDialog';
import { MCPServerDialog } from '@/components/MCPServerDialog';

type Tab = 'tools' | 'skills';

// ── Stats Bar ────────────────────────────────────────────────────────────────

function StatBox({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="sera-card-static px-4 py-3 text-center">
      <div className="text-lg font-bold text-sera-text">{value}</div>
      <div className="text-[10px] uppercase tracking-wider text-sera-text-dim">{label}</div>
    </div>
  );
}

// ── Main Page ────────────────────────────────────────────────────────────────

export default function ToolsPage() {
  const { data: tools, isLoading: toolsLoading } = useTools();
  const { data: skills, isLoading: skillsLoading } = useSkills();
  const deleteSkill = useDeleteSkill();
  const [tab, setTab] = useState<Tab>('tools');
  const [search, setSearch] = useState('');

  // Dialog state
  const [showEditor, setShowEditor] = useState(false);
  const [showImport, setShowImport] = useState(false);
  const [showMCP, setShowMCP] = useState(false);
  const [editingSkill, setEditingSkill] = useState<GuidanceSkillInfo | undefined>();

  function handleEditSkill(skill: GuidanceSkillInfo) {
    setEditingSkill(skill);
    setShowEditor(true);
  }

  function handleDeleteSkill(name: string) {
    deleteSkill.mutate(name, {
      onSuccess: () => toast.success(`Skill "${name}" deleted`),
      onError: (err) => toast.error(err instanceof Error ? err.message : 'Delete failed'),
    });
  }

  // Stats
  const builtinCount = useMemo(
    () => (tools ?? []).filter((t) => t.source === 'builtin').length,
    [tools]
  );
  const mcpCount = useMemo(() => (tools ?? []).filter((t) => t.source === 'mcp').length, [tools]);
  const mcpServers = useMemo(() => {
    const servers = new Set<string>();
    for (const t of tools ?? []) {
      if (t.server) servers.add(t.server);
    }
    return servers.size;
  }, [tools]);

  // Filtering
  const filteredTools = useMemo(() => {
    if (!search) return tools ?? [];
    const lower = search.toLowerCase();
    return (tools ?? []).filter(
      (t) =>
        t.id.toLowerCase().includes(lower) ||
        (t.description?.toLowerCase().includes(lower) ?? false) ||
        (t.server?.toLowerCase().includes(lower) ?? false)
    );
  }, [tools, search]);

  const filteredSkills = useMemo(() => {
    if (!search) return skills ?? [];
    const lower = search.toLowerCase();
    return (skills ?? []).filter(
      (s) =>
        s.name.toLowerCase().includes(lower) ||
        (s.description?.toLowerCase().includes(lower) ?? false) ||
        (s.category?.toLowerCase().includes(lower) ?? false) ||
        (s.tags?.some((t) => t.toLowerCase().includes(lower)) ?? false)
    );
  }, [skills, search]);

  // Group tools by source
  const groupedTools = useMemo(() => {
    const groups = new Map<string, ToolInfo[]>();
    for (const tool of filteredTools) {
      const key =
        tool.source === 'builtin'
          ? 'Builtin Tools'
          : tool.server
            ? `MCP: ${tool.server}`
            : 'Custom Tools';
      const list = groups.get(key);
      if (list) {
        list.push(tool);
      } else {
        groups.set(key, [tool]);
      }
    }
    return groups;
  }, [filteredTools]);

  // Group skills by category
  const groupedSkills = useMemo(() => {
    const groups = new Map<string, GuidanceSkillInfo[]>();
    for (const skill of filteredSkills) {
      const key = skill.category ?? 'Uncategorized';
      const list = groups.get(key);
      if (list) {
        list.push(skill);
      } else {
        groups.set(key, [skill]);
      }
    }
    return groups;
  }, [filteredSkills]);

  const isLoading = tab === 'tools' ? toolsLoading : skillsLoading;

  return (
    <div className="p-6">
      {/* Header */}
      <div className="sera-page-header">
        <h1 className="sera-page-title">Tools & Skills</h1>
      </div>
      <p className="text-sm text-sera-text-muted mb-6">
        Tools are executable functions agents invoke during reasoning. Skills are guidance documents
        injected into agent context.
      </p>

      {/* Tabs + Search */}
      <div className="flex items-center justify-between mb-6 gap-4 flex-wrap">
        <div className="flex items-center gap-1 bg-sera-surface rounded-lg p-1">
          <button
            onClick={() => setTab('tools')}
            className={cn(
              'flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-colors',
              tab === 'tools'
                ? 'bg-sera-accent text-sera-bg'
                : 'text-sera-text-muted hover:text-sera-text'
            )}
          >
            <Wrench size={12} />
            Tools
            {tools && <span className="ml-1 text-[10px] opacity-70">{tools.length}</span>}
          </button>
          <button
            onClick={() => setTab('skills')}
            className={cn(
              'flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium transition-colors',
              tab === 'skills'
                ? 'bg-sera-accent text-sera-bg'
                : 'text-sera-text-muted hover:text-sera-text'
            )}
          >
            <BookOpen size={12} />
            Skills
            {skills && <span className="ml-1 text-[10px] opacity-70">{skills.length}</span>}
          </button>
        </div>

        <div className="flex items-center gap-2">
          <div className="relative">
            <Search
              size={13}
              className="absolute left-3 top-1/2 -translate-y-1/2 text-sera-text-dim"
            />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={tab === 'tools' ? 'Search tools…' : 'Search skills…'}
              className="pl-8 pr-3 py-1.5 bg-sera-surface border border-sera-border rounded-lg text-xs text-sera-text outline-none focus:border-sera-accent placeholder:text-sera-text-dim w-60"
            />
          </div>

          {/* Action buttons */}
          {tab === 'tools' ? (
            <Button size="sm" variant="outline" onClick={() => setShowMCP(true)}>
              <Plus size={12} /> MCP Server
            </Button>
          ) : (
            <>
              <Button size="sm" variant="outline" onClick={() => setShowImport(true)}>
                <Download size={12} /> Import
              </Button>
              <Button
                size="sm"
                onClick={() => {
                  setEditingSkill(undefined);
                  setShowEditor(true);
                }}
              >
                <Plus size={12} /> Create Skill
              </Button>
            </>
          )}
        </div>
      </div>

      {/* Tools Tab */}
      {tab === 'tools' && (
        <>
          {/* Stats */}
          {!toolsLoading && tools && tools.length > 0 && (
            <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-6">
              <StatBox label="Total Tools" value={tools.length} />
              <StatBox label="Builtin" value={builtinCount} />
              <StatBox label="MCP Tools" value={mcpCount} />
              <StatBox label="MCP Servers" value={mcpServers} />
            </div>
          )}

          {isLoading ? (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              {[1, 2, 3, 4, 5, 6].map((i) => (
                <Skeleton key={i} className="h-32 rounded-xl" />
              ))}
            </div>
          ) : filteredTools.length === 0 ? (
            <EmptyState
              icon={<Wrench size={24} />}
              title={search ? 'No matching tools' : 'No tools registered'}
              description={
                search
                  ? 'Try a different search term.'
                  : 'Tools appear here once builtin skills are registered or MCP servers are connected.'
              }
            />
          ) : (
            <div className="space-y-6">
              {[...groupedTools.entries()].map(([group, groupTools]) => (
                <div key={group}>
                  <h3 className="text-[10px] font-bold uppercase tracking-wider text-sera-text-dim mb-3 flex items-center gap-2">
                    {group.startsWith('MCP:') ? (
                      <Server size={11} className="text-sera-accent" />
                    ) : (
                      <Cpu size={11} className="text-sera-text-dim" />
                    )}
                    {group}
                    <span className="text-sera-text-dim/50">({groupTools.length})</span>
                  </h3>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                    {groupTools.map((tool) => (
                      <ToolCard key={tool.id} tool={tool} />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      )}

      {/* Skills Tab */}
      {tab === 'skills' && (
        <>
          {/* Stats */}
          {!skillsLoading && skills && skills.length > 0 && (
            <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-6">
              <StatBox label="Total Skills" value={skills.length} />
              <StatBox
                label="Categories"
                value={new Set(skills.map((s) => s.category ?? 'Uncategorized')).size}
              />
              <StatBox
                label="Total Tokens"
                value={skills.reduce((sum, s) => sum + (s.maxTokens ?? 0), 0).toLocaleString()}
              />
              <StatBox
                label="In Use"
                value={skills.filter((s) => s.usedBy && s.usedBy.length > 0).length}
              />
            </div>
          )}

          {isLoading ? (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              {[1, 2, 3].map((i) => (
                <Skeleton key={i} className="h-32 rounded-xl" />
              ))}
            </div>
          ) : filteredSkills.length === 0 ? (
            <EmptyState
              icon={<BookOpen size={24} />}
              title={search ? 'No matching skills' : 'No guidance skills loaded'}
              description={
                search
                  ? 'Try a different search term.'
                  : 'Place markdown files with YAML frontmatter in skills/builtin/ to register guidance skills.'
              }
            />
          ) : (
            <div className="space-y-6">
              {[...groupedSkills.entries()].map(([category, categorySkills]) => (
                <div key={category}>
                  <h3 className="text-[10px] font-bold uppercase tracking-wider text-sera-text-dim mb-3 flex items-center gap-2">
                    <BookOpen size={11} className="text-sera-accent" />
                    {category}
                    <span className="text-sera-text-dim/50">({categorySkills.length})</span>
                  </h3>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
                    {categorySkills.map((skill) => (
                      <SkillCard
                        key={skill.id}
                        skill={skill}
                        onEdit={handleEditSkill}
                        onDelete={handleDeleteSkill}
                      />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      )}

      {/* Dialogs */}
      <SkillEditorDialog
        open={showEditor}
        onOpenChange={(open) => {
          setShowEditor(open);
          if (!open) setEditingSkill(undefined);
        }}
        initial={
          editingSkill
            ? {
                name: editingSkill.name,
                version: editingSkill.version ?? '1.0.0',
                description: editingSkill.description ?? '',
                triggers: editingSkill.triggers ?? [],
                category: editingSkill.category,
                tags: editingSkill.tags,
                maxTokens: editingSkill.maxTokens,
                content: '', // Content needs to be fetched separately
              }
            : undefined
        }
      />
      <SkillImportDialog open={showImport} onOpenChange={setShowImport} />
      <MCPServerDialog open={showMCP} onOpenChange={setShowMCP} />
    </div>
  );
}
