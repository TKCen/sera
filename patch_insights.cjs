const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'web/src/pages/InsightsPage.tsx');
let content = fs.readFileSync(filePath, 'utf8');

if (!content.includes("import { ErrorBoundary }")) {
    content = content.replace("import { Spinner } from '@/components/ui/spinner';", "import { Spinner } from '@/components/ui/spinner';\nimport { ErrorBoundary } from '@/components/ErrorBoundary';");
}

content = content.replace(
    "const { data, isLoading, refetch, isFetching } = useUsage({ groupBy: 'agent', from, to });",
    "const { data, isLoading, isError, error, refetch, isFetching } = useUsage({ groupBy: 'agent', from, to });"
);

content = content.replace(
    `      {isLoading ? (`,
    `      <ErrorBoundary fallbackMessage="Failed to load insights.">
        {isLoading ? (`
);

content = content.replace(
    `          <p className="text-sm text-sera-text-muted">Loading usage data...</p>
        </div>
      ) : (
        <>`,
    `          <p className="text-sm text-sera-text-muted">Loading usage data...</p>
        </div>
      ) : isError ? (
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

content = content.replace(
    `          )}
        </>
      )}
    </div>`,
    `          )}
        </>
      )}
      </ErrorBoundary>
    </div>`
);

fs.writeFileSync(filePath, content, 'utf8');
console.log('InsightsPage patched.');
