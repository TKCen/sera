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
    this.props.onReset?.();
  };

  render(): ReactNode {
    if (this.state.hasError) {
      return (
        <div className="flex flex-col items-center justify-center min-h-[60vh] p-6 text-center">
          <div className="bg-sera-surface border border-sera-border rounded-xl p-8 max-w-md w-full shadow-2xl animate-in fade-in zoom-in duration-300">
            <div className="w-12 h-12 bg-sera-error/10 rounded-full flex items-center justify-center mx-auto mb-4">
              <AlertCircle className="text-sera-error" size={24} />
            </div>
            <h2 className="text-xl font-semibold text-sera-text mb-2">Application Error</h2>
            <p className="text-sm text-sera-text-muted mb-6">
              {this.props.fallbackMessage ||
                'An unexpected error occurred that prevented the application from continuing.'}
            </p>

            {this.state.error && (
              <div className="bg-sera-bg/50 border border-sera-border/50 rounded-lg p-4 mb-8 overflow-x-auto text-left group relative">
                <div className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity">
                  <span className="text-[10px] text-sera-text-dim bg-sera-surface px-1.5 py-0.5 rounded border border-sera-border">
                    Debug
                  </span>
                </div>
                <p className="text-xs text-sera-text-dim font-mono break-all leading-relaxed">
                  {this.state.error.name}: {this.state.error.message}
                </p>
              </div>
            )}

            <div className="flex flex-col gap-3">
              <Button onClick={this.handleReset} className="w-full">
                <RefreshCcw size={14} className="mr-2" />
                Try Again
              </Button>
              <Button onClick={this.handleReload} variant="outline" className="w-full">
                Reload Application
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="text-sera-text-dim hover:text-sera-text"
                onClick={() => (window.location.href = '/')}
              >
                Go to Dashboard
              </Button>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
