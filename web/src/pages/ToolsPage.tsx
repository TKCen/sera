import { useState, useMemo } from 'react';
import { toast } from 'sonner';
import {
  Wrench,
  BookOpen,
  Search,
  ChevronDown,
  ChevronRight,
  Shield,
  Cpu,
  Server,
  Tag,
  Zap,
  Bot,
  Plus,
  Download,
  Edit2,
  Trash2,
} from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { EmptyState } from '@/components/EmptyState';
import { useTools } from '@/hooks/useTools';
import { useSkills, useDeleteSkill } from '@/hooks/useSkills';
import type { ToolInfo, GuidanceSkillInfo } from '@/lib/api/types';
import { cn } from '@/lib/utils';
import { SkillEditorDialog } from '@/components/SkillEditorDialog';
import { SkillImportDialog } from '@/components/SkillImportDialog';
import { MCPServerDialog } from '@/components/MCPServerDialog';

type Tab = 'tools' | 'skills';

// ── Tool Card ────────────────────────────────────────────────────────────────

function ToolCard({ tool }: { tool: ToolInfo }) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div className="sera-card p-4 flex flex-col gap-2">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="font-mono text-sm font-medium text-sera-text">{tool.id}</span>
            <Badge variant={tool.source === 'builtin' ? 'default' : 'accent'}>{tool.source}</Badge>
          </div>
          {tool.server && (
            <span className="text-[10px] text-sera-text-dim flex items-center gap-1 mt-0.5">
              <Server size={9} /> {tool.server}
            </span>
          )}
        </div>
        <div className="flex items-center gap-1.5 flex-shrink-0">
          {tool.minTier != null && (
            <span className="text-[10px] px-1.5 py-0.5 rounded bg-sera-surface-hover text-sera-text-dim flex items-center gap-1">
              <Shield size={9} /> Tier {tool.minTier}+
            </span>
          )}
        </div>
      </div>

      {tool.description && (
        <p className="text-xs text-sera-text-muted line-clamp-2">{tool.description}</p>
      )}

      {tool.capabilityRequired && (
        <div className="flex items-center gap-1 text-[10px] text-amber-400">
          <Zap size={9} /> Requires{' '}
          <code className="bg-sera-bg px-1 rounded">{tool.capabilityRequired}</code>
        </div>
      )}

      {/* Parameters */}
      {tool.parameters && tool.parameters.length > 0 && (
        <div>
          <button
            onClick={() => setExpanded(!expanded)}
            className="text-[10px] text-sera-text-dim hover:text-sera-text flex items-center gap-1 transition-colors"
          >
            {expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
            {tool.parameters.length} parameter{tool.parameters.length !== 1 ? 's' : ''}
          </button>
          {expanded && (
            <div className="mt-1.5 space-y-1 pl-3 border-l border-sera-border/50">
              {tool.parameters.map((p) => (
                <div key={p.name} className="text-[10px]">
                  <span className="font-mono text-sera-accent">{p.name}</span>
                  <span className="text-sera-text-dim ml-1">({p.type})</span>
                  {p.required && <span className="text-amber-400 ml-1">*</span>}
                  {p.description && (
                    <span className="text-sera-text-dim ml-1.5">— {p.description}</span>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Used by */}
      {tool.usedBy && tool.usedBy.length > 0 && (
        <div className="flex items-center gap-1.5 flex-wrap mt-1">
          <Bot size={10} className="text-sera-text-dim flex-shrink-0" />
          {tool.usedBy.map((agent: string) => (
            <span
              key={agent}
              className="text-[10px] px-1.5 py-0.5 rounded bg-sera-surface-hover text-sera-text-muted"
            >
              {agent}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

// ── Skill Card ───────────────────────────────────────────────────────────────

function SkillCard({
  skill,
  onEdit,
  onDelete,
}: {
  skill: GuidanceSkillInfo;
  onEdit: (skill: GuidanceSkillInfo) => void;
  onDelete: (name: string) => void;
}) {
  return (
    <div className="sera-card p-4 flex flex-col gap-2 group">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 flex-wrap">
            <span className="font-medium text-sm text-sera-text">{skill.name}</span>
            {skill.version && <Badge variant="default">v{skill.version}</Badge>}
          </div>
        </div>
        <div className="flex items-center gap-1.5 flex-shrink-0">
          {skill.source && (
            <Badge variant={skill.source === 'bundled' ? 'default' : 'accent'}>
              {skill.source}
            </Badge>
          )}
        </div>
      </div>

      {skill.description && (
        <p className="text-xs text-sera-text-muted line-clamp-2">{skill.description}</p>
      )}

      {/* Category */}
      {skill.category && (
        <div className="flex items-center gap-1 text-[10px] text-sera-text-dim">
          <Tag size={9} /> {skill.category}
        </div>
      )}

      {/* Tags */}
      {skill.tags && skill.tags.length > 0 && (
        <div className="flex items-center gap-1 flex-wrap">
          {skill.tags.map((tag: string) => (
            <span
              key={tag}
              className="text-[9px] px-1.5 py-0.5 rounded bg-sera-accent/10 text-sera-accent"
            >
              {tag}
            </span>
          ))}
        </div>
      )}

      {/* Triggers */}
      {skill.triggers && skill.triggers.length > 0 && (
        <div className="flex items-center gap-1 flex-wrap text-[10px] text-sera-text-dim">
          <Zap size={9} className="flex-shrink-0" />
          {skill.triggers.map((t: string) => (
            <code key={t} className="px-1 py-0.5 rounded bg-sera-bg text-sera-text-muted">
              {t}
            </code>
          ))}
        </div>
      )}

      {/* Token cost + used by + actions */}
      <div className="flex items-center justify-between mt-auto pt-2 text-[10px] text-sera-text-dim">
        <div className="flex items-center gap-2">
          {skill.maxTokens != null && skill.maxTokens > 0 && (
            <span className="flex items-center gap-1">
              <Cpu size={9} /> ~{skill.maxTokens.toLocaleString()} tok
            </span>
          )}
          {skill.usedBy && skill.usedBy.length > 0 && (
            <span className="flex items-center gap-1">
              <Bot size={9} /> {skill.usedBy.length} agent{skill.usedBy.length !== 1 ? 's' : ''}
            </span>
          )}
        </div>
        <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
          <button
            onClick={() => onEdit(skill)}
            className="p-1 hover:text-sera-accent transition-colors"
            title="Edit skill"
          >
            <Edit2 size={11} />
          </button>
          <button
            onClick={() => onDelete(skill.name)}
            className="p-1 hover:text-red-400 transition-colors"
            title="Delete skill"
          >
            <Trash2 size={11} />
          </button>
        </div>
      </div>
    </div>
  );
}

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
    if (!confirm(`Delete skill "${name}"?`)) return;
    deleteSkill.mutate(name, {
      onSuccess: () => toast.success(`Skill "${name}" deleted`),
      onError: (err) => toast.error(err instanceof Error ? err.message : 'Delete failed'),
    });
  }

  // Stats
  const builtinCount = useMemo(
    () => (tools ?? []).filter((t: ToolInfo) => t.source === 'builtin').length,
    [tools]
  );
  const mcpCount = useMemo(
    () => (tools ?? []).filter((t: ToolInfo) => t.source === 'mcp').length,
    [tools]
  );
  const mcpServers = useMemo(() => {
    const servers = new Set<string>();
    for (const t of (tools ?? []) as ToolInfo[]) {
      if (t.server) servers.add(t.server);
    }
    return servers.size;
  }, [tools]);

  // Filtering
  const filteredTools = useMemo(() => {
    if (!search) return (tools ?? []) as ToolInfo[];
    const lower = search.toLowerCase();
    return ((tools ?? []) as ToolInfo[]).filter(
      (t: ToolInfo) =>
        t.id.toLowerCase().includes(lower) ||
        (t.description?.toLowerCase().includes(lower) ?? false) ||
        (t.server?.toLowerCase().includes(lower) ?? false)
    );
  }, [tools, search]);

  const filteredSkills = useMemo(() => {
    if (!search) return (skills ?? []) as GuidanceSkillInfo[];
    const lower = search.toLowerCase();
    return ((skills ?? []) as GuidanceSkillInfo[]).filter(
      (s: GuidanceSkillInfo) =>
        s.name.toLowerCase().includes(lower) ||
        (s.description?.toLowerCase().includes(lower) ?? false) ||
        (s.category?.toLowerCase().includes(lower) ?? false) ||
        (s.tags?.some((t: string) => t.toLowerCase().includes(lower)) ?? false)
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
              {[1, 2, 3, 4, 5, 6].map((i: number) => (
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
              {[1, 2, 3].map((i: number) => (
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
