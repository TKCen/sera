const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'web/src/pages/InsightsPage.tsx');
let content = fs.readFileSync(filePath, 'utf8');

console.log(content.indexOf('isError'));

content = content.replace(
    `      ) : (
        <>`,
    `      ) : isError ? (
        <div className="flex flex-col items-center justify-center py-24 gap-4 bg-sera-surface border border-sera-error/20 rounded-xl">
          <p className="text-sm text-sera-error font-medium">Failed to load insights</p>
          <div className="text-xs text-sera-text-dim max-w-md text-center">
            {error instanceof Error ? error.message : String(error)}
          </div>
          <button
            onClick={() => void refetch()}
            className="px-4 py-2 mt-2 bg-sera-surface-hover hover:bg-sera-surface-active text-sera-text border border-sera-border rounded-lg text-sm font-medium transition-all flex items-center gap-2"
          >
            <RefreshCw size={14} />
            Retry
          </button>
        </div>
      ) : (
        <>`
);

fs.writeFileSync(filePath, content, 'utf8');
console.log('InsightsPage patched again.');
