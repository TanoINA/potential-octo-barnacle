import { Component, Suspense, type ReactNode } from 'react';
import { useModalAccessibility } from '../hooks/useModalAccessibility';

interface OptionalPanelProps {
  label: string;
  onClose: () => void;
  children: ReactNode;
}

function PanelFallback({ label, onClose, failed = false }: Omit<OptionalPanelProps, 'children'> & { failed?: boolean }) {
  const dialogRef = useModalAccessibility<HTMLDivElement>(true, onClose);

  return (
    <div
      className="ui-overlay-enter fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      onClick={(event) => { if (event.target === event.currentTarget) onClose(); }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={`${label}: ${failed ? 'unavailable' : 'loading'}`}
        tabIndex={-1}
        className="ui-panel-enter rounded-lg border border-[#30363d] bg-[#161b22] p-4 text-sm text-[#e6edf3] shadow-lg"
      >
        <p role={failed ? 'alert' : 'status'}>
          {failed ? `Could not load ${label}. Close this panel and reload the app before trying again.` : `Loading ${label}…`}
        </p>
        <button type="button" onClick={onClose} aria-label={`Close ${label}`} className="mt-3 rounded border border-[#30363d] px-3 py-1">
          Close
        </button>
      </div>
    </div>
  );
}

// Keep a failed optional chunk from replacing the entire chat with the global error screen.
// React.lazy caches rejected imports, so do not offer a misleading in-place retry.
class PanelErrorBoundary extends Component<OptionalPanelProps, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    if (this.state.failed) return <PanelFallback label={this.props.label} onClose={this.props.onClose} failed />;
    return this.props.children;
  }
}

export function OptionalPanel({ children, ...props }: OptionalPanelProps) {
  return (
    <PanelErrorBoundary {...props}>
      <Suspense fallback={<PanelFallback {...props} />}>{children}</Suspense>
    </PanelErrorBoundary>
  );
}
