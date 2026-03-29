import React, { Component, ReactNode } from 'react';

interface ErrorBoundaryProps {
  children: ReactNode;
  fallbackMessage?: string;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo): void {
    console.error('[ErrorBoundary] caught an error:', error, errorInfo);
  }

  handleReload = (): void => {
    window.location.reload();
  };

  render(): ReactNode {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center min-h-[50vh] p-6 text-center">
          <div className="bg-sera-surface border border-sera-border rounded-xl p-8 max-w-md w-full shadow-lg">
            <h2 className="text-xl font-semibold text-sera-error mb-2">Something went wrong</h2>
            <p className="text-sm text-sera-text-muted mb-4">
              {this.props.fallbackMessage || 'An unexpected error occurred.'}
            </p>
            {this.state.error && (
              <div className="bg-sera-bg border border-sera-border rounded-lg p-3 mb-6 overflow-x-auto text-left">
                <p className="text-xs text-sera-text-dim font-mono break-all">
                  {this.state.error.message}
                </p>
              </div>
            )}
            <button
              onClick={this.handleReload}
              className="px-4 py-2 bg-sera-accent text-sera-bg rounded-lg text-sm font-medium hover:brightness-110 transition-all"
            >
              Reload Page
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
