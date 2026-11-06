import { Component, type ErrorInfo, type ReactNode } from 'react';
import { AlertTriangle, RefreshCw } from 'lucide-react';

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    console.error('Unhandled React Error in ErrorBoundary:', error, errorInfo);
  }

  handleReload = () => {
    window.location.reload();
  };

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-screen w-screen flex-col items-center justify-center bg-[#0d1117] p-6 text-[#e6edf3]">
          <div className="max-w-md w-full rounded-2xl border border-[#30363d] bg-[#161b22] p-6 text-center shadow-2xl">
            <div className="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-xl border border-[#f85149]/40 bg-[#f85149]/10 text-[#f85149]">
              <AlertTriangle size={24} />
            </div>
            <h2 className="text-base font-semibold text-[#e6edf3]">Something went wrong</h2>
            <p className="mt-2 text-xs leading-relaxed text-[#8b949e]">
              An unexpected interface error occurred. You can reload the app to restore state.
            </p>
            {this.state.error?.message && (
              <div className="my-3 max-h-40 overflow-y-auto rounded border border-[#30363d] bg-[#0d1117] p-2 text-left font-mono text-[11px] text-[#f85149]">
                <div>{this.state.error.message}</div>
                {this.state.error.stack && (
                  <pre className="mt-2 text-[10px] text-[#8b949e] whitespace-pre-wrap">{this.state.error.stack}</pre>
                )}
              </div>
            )}
            <button
              type="button"
              onClick={this.handleReload}
              className="inline-flex items-center gap-2 rounded-lg bg-[#238636] px-4 py-2 text-xs font-medium text-white hover:bg-[#2ea043] transition-colors"
            >
              <RefreshCw size={13} />
              <span>Reload Application</span>
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
