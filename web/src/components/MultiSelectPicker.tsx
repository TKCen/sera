import { useState, useMemo } from 'react';
import { Search, AlertTriangle } from 'lucide-react';
import { cn } from '@/lib/utils';

export interface PickerItem {
  id: string;
  label: string;
  description?: string;
  group?: string;
  badge?: string;
  badgeVariant?: 'default' | 'accent' | 'warning';
  warning?: string;
  tokenCost?: number;
}

interface MultiSelectPickerProps {
  items: PickerItem[];
  selected: string[];
  onChange: (selected: string[]) => void;
  placeholder?: string;
  maxHeight?: string;
  loading?: boolean;
}

const BADGE_STYLES: Record<string, string> = {
  default: 'bg-sera-surface-hover text-sera-text-muted',
  accent: 'bg-sera-accent/15 text-sera-accent',
  warning: 'bg-amber-500/15 text-amber-400',
};

export function MultiSelectPicker({
  items,
  selected,
  onChange,
  placeholder = 'Search…',
  maxHeight = '240px',
  loading = false,
}: MultiSelectPickerProps) {
  const [search, setSearch] = useState('');

  const filtered = useMemo(() => {
    if (!search) return items;
    const lower = search.toLowerCase();
    return items.filter(
      (item) =>
        item.id.toLowerCase().includes(lower) ||
        item.label.toLowerCase().includes(lower) ||
        (item.description?.toLowerCase().includes(lower) ?? false)
    );
  }, [items, search]);

  // Group items by their group field
  const grouped = useMemo(() => {
    const groups = new Map<string, PickerItem[]>();
    for (const item of filtered) {
      const key = item.group ?? '';
      const list = groups.get(key);
      if (list) {
        list.push(item);
      } else {
        groups.set(key, [item]);
      }
    }
    return groups;
  }, [filtered]);

  function toggle(id: string) {
    if (selected.includes(id)) {
      onChange(selected.filter((s) => s !== id));
    } else {
      onChange([...selected, id]);
    }
  }

  function renderItem(item: PickerItem) {
    const isSelected = selected.includes(item.id);
    return (
      <label
        key={item.id}
        className={cn(
          'flex items-start gap-2.5 px-3 py-2 cursor-pointer transition-colors border-b border-sera-border/30 last:border-0',
          isSelected ? 'bg-sera-accent/5' : 'hover:bg-sera-surface-hover'
        )}
      >
        <input
          type="checkbox"
          checked={isSelected}
          onChange={() => toggle(item.id)}
          className="accent-sera-accent mt-0.5 flex-shrink-0"
        />
        <div className="min-w-0 flex-1">
          <span className="text-xs font-mono text-sera-text flex items-center gap-1.5">
            <span className="truncate">{item.label}</span>
            {item.badge && (
              <span
                className={cn(
                  'text-[9px] px-1.5 py-0.5 rounded font-sans font-medium flex-shrink-0',
                  BADGE_STYLES[item.badgeVariant ?? 'default']
                )}
              >
                {item.badge}
              </span>
            )}
            {item.tokenCost != null && item.tokenCost > 0 && (
              <span className="text-[9px] text-sera-text-dim font-sans flex-shrink-0">
                ~{item.tokenCost} tok
              </span>
            )}
          </span>
          {item.description && (
            <span className="text-[10px] text-sera-text-dim block truncate">
              {item.description}
            </span>
          )}
          {item.warning && (
            <span className="text-[10px] text-amber-400 flex items-center gap-1 mt-0.5">
              <AlertTriangle size={9} />
              {item.warning}
            </span>
          )}
        </div>
      </label>
    );
  }

  const hasGroups = filtered.some((item) => item.group);

  return (
    <div className="border border-sera-border rounded-lg overflow-hidden bg-sera-surface">
      {/* Search */}
      <div className="flex items-center gap-2 px-3 py-2 border-b border-sera-border bg-sera-bg/50">
        <Search size={12} className="text-sera-text-dim flex-shrink-0" />
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={placeholder}
          className="flex-1 bg-transparent text-xs text-sera-text outline-none placeholder:text-sera-text-dim"
        />
        {selected.length > 0 && (
          <span className="text-[10px] text-sera-accent font-medium flex-shrink-0">
            {selected.length} selected
          </span>
        )}
      </div>

      {/* Items */}
      <div className="overflow-y-auto" style={{ maxHeight }}>
        {loading ? (
          <div className="px-3 py-4 text-xs text-sera-text-dim text-center">Loading…</div>
        ) : filtered.length === 0 ? (
          <div className="px-3 py-4 text-xs text-sera-text-dim text-center">
            {items.length === 0 ? 'No items available' : 'No matches'}
          </div>
        ) : hasGroups ? (
          [...grouped.entries()].map(([groupName, groupItems]) => (
            <div key={groupName}>
              {groupName && (
                <div className="px-3 py-1.5 bg-sera-bg/70 border-b border-sera-border/50 sticky top-0 z-10">
                  <span className="text-[9px] font-bold uppercase tracking-wider text-sera-text-dim">
                    {groupName}
                  </span>
                </div>
              )}
              {groupItems.map(renderItem)}
            </div>
          ))
        ) : (
          filtered.map(renderItem)
        )}
      </div>
    </div>
  );
}
