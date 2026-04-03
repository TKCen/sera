import React, { Component, ReactNode } from 'react';
import { AlertCircle, RefreshCcw } from 'lucide-react';
import { Button } from '@/components/ui/button';

interface ErrorBoundaryProps {
  children: ReactNode;
  fallbackMessage?: string;
  onReset?: () => void;
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
    console.error('ErrorBoundary caught an error:', error, errorInfo);
  }

  handleReload = (): void => {
    window.location.reload();
  };

  handleReset = (): void => {
    this.setState({ hasError: false, error: null });
    if (this.props.onReset) {
      this.props.onReset();
    }
  };

  render(): ReactNode {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center min-h-[40vh] p-6 text-center">
          <div className="sera-card-static p-8 max-w-md w-full shadow-2xl animate-in fade-in zoom-in duration-300">
            <div className="w-12 h-12 bg-sera-error/10 rounded-full flex items-center justify-center mx-auto mb-4">
              <AlertCircle className="text-sera-error" size={24} />
            </div>
            <h2 className="text-lg font-semibold text-sera-text mb-2 tracking-tight">
              An error occurred
            </h2>
            <p className="text-xs text-sera-text-muted mb-6 leading-relaxed">
              {this.props.fallbackMessage ||
                'An unexpected error occurred in this component. You can try resetting it or reloading the page.'}
            </p>

            {this.state.error && (
              <div className="bg-sera-bg border border-sera-border rounded-lg p-3 mb-6 overflow-x-auto text-left group relative">
                <div className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity">
                  <span className="text-[9px] text-sera-text-dim bg-sera-surface px-1.5 py-0.5 rounded border border-sera-border uppercase font-bold tracking-wider">
                    Stack Trace
                  </span>
                </div>
                <p className="text-[10px] text-sera-text-dim font-mono break-all leading-relaxed">
                  {this.state.error.name}: {this.state.error.message}
                </p>
              </div>
            )}

            <div className="flex flex-col gap-2.5">
              <Button onClick={this.handleReset} className="w-full py-5" size="sm">
                <RefreshCcw size={14} className="mr-2" />
                Try Again
              </Button>
              <div className="grid grid-cols-2 gap-2">
                <Button onClick={this.handleReload} variant="outline" size="sm">
                  Reload Page
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => (window.location.href = '/')}
                >
                  Dashboard
                </Button>
              </div>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
