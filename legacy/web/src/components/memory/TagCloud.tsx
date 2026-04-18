interface TagCloudProps {
  tags: Array<{ tag: string; count: number }>;
  activeTag?: string;
  onTagClick: (tag: string) => void;
}

export function TagCloud({ tags, activeTag, onTagClick }: TagCloudProps) {
  if (tags.length === 0) return null;

  const maxCount = Math.max(...tags.map((t) => t.count));
  const minSize = 12;
  const maxSize = 24;

  return (
    <div className="flex flex-wrap gap-1.5">
      {tags.map(({ tag, count }) => {
        const size =
          maxCount > 1 ? minSize + ((count - 1) / (maxCount - 1)) * (maxSize - minSize) : minSize;
        const isActive = activeTag === tag;
        return (
          <button
            key={tag}
            type="button"
            onClick={() => onTagClick(isActive ? '' : tag)}
            className={`transition-colors rounded px-1.5 py-0.5 ${
              isActive
                ? 'bg-sera-accent/20 text-sera-accent'
                : 'text-sera-text-muted hover:text-sera-text hover:bg-sera-surface'
            }`}
            style={{ fontSize: `${size}px` }}
            title={`${tag} (${count})`}
          >
            {tag}
          </button>
        );
      })}
    </div>
  );
}
