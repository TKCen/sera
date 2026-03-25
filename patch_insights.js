const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'web/src/pages/InsightsPage.tsx');
let content = fs.readFileSync(filePath, 'utf8');

// Import ErrorBoundary
if (!content.includes("import { ErrorBoundary }")) {
    content = content.replace("import { Spinner } from '@/components/ui/spinner';", "import { Spinner } from '@/components/ui/spinner';\nimport { ErrorBoundary } from '@/components/ErrorBoundary';");
}

// Add isError and error to useUsage destructuring
content = content.replace(
    "const { data, isLoading, refetch, isFetching } = useUsage({ groupBy: 'agent', from, to });",
    "const { data, isLoading, isError, error, refetch, isFetching } = useUsage({ groupBy: 'agent', from, to });"
);

// Add the ErrorBoundary wrap and the Error State handling
const renderContentOld = `      {isLoading ? (
        <div className="flex flex-col items-center justify-center py-24 gap-4">
          <Spinner size="md" className="text-sera-accent" />
          <p className="text-sm text-sera-text-muted">Loading usage data...</p>
        </div>
      ) : (`

const renderContentNew = `      <ErrorBoundary fallbackMessage="The insights dashboard encountered an error.">
        {isLoading ? (
          <div className="flex flex-col items-center justify-center py-24 gap-4">
            <Spinner size="md" className="text-sera-accent" />
            <p className="text-sm text-sera-text-muted">Loading usage data...</p>
          </div>
        ) : isError ? (
          <div className="flex flex-col items-center justify-center py-24 gap-4 bg-sera-surface border border-sera-border rounded-xl">
            <p className="text-sm text-sera-error font-medium">Failed to load insights</p>
            {error && (
              <p className="text-xs text-sera-text-dim max-w-md text-center">
                {error instanceof Error ? error.message : String(error)}
              </p>
            )}
            <button
              onClick={() => void refetch()}
              className="px-4 py-2 mt-2 bg-sera-accent text-sera-bg rounded-lg text-sm font-medium hover:brightness-110 transition-all flex items-center gap-2"
            >
              <RefreshCw size={14} />
              Retry
            </button>
          </div>
        ) : (`

content = content.replace(renderContentOld, renderContentNew);

const closeWrapOld = `          )}
        </>
      )}
    </div>
  );
}`;

const closeWrapNew = `          )}
        </>
      )}
      </ErrorBoundary>
    </div>
  );
}`;

content = content.replace(closeWrapOld, closeWrapNew);

fs.writeFileSync(filePath, content, 'utf8');
console.log('InsightsPage patched.');
