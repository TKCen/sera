import { Tag, Zap, Cpu, Bot, Edit2, Trash2 } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import type { GuidanceSkillInfo } from '@/lib/api/types';

export function SkillCard({
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
          {skill.tags.map((tag) => (
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
          {skill.triggers.map((t) => (
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
