import { memo, useCallback, useEffect, useMemo, useState } from 'react';
import {
  Archive,
  Check,
  Code,
  Copy,
  ExternalLink,
  FileCode,
  FileText,
  GitCommit,
  Layers,
  Search,
  Terminal,
  X,
} from 'lucide-react';
import type { ArtifactType, GlobalArtifactItem } from '../types/agent';
import { useModalAccessibility } from '../hooks/useModalAccessibility';
import { MarkdownRenderer } from './MarkdownRenderer';
import { SideBySideDiff } from './SideBySideDiff';

interface ArtifactsLibraryModalProps {
  open: boolean;
  onClose: () => void;
  onLoadArtifacts: () => Promise<GlobalArtifactItem[]>;
  onSelectSession: (sessionId: string) => void;
}

type FilterCategory = 'all' | 'code' | 'diff' | 'markdown' | 'file' | 'terminal';

function formatTimestamp(ts: number): string {
  if (!ts) return '';
  const date = new Date(ts);
  return date.toLocaleDateString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

function getArtifactIcon(type: ArtifactType) {
  switch (type) {
    case 'code':
      return <Code size={13} className="text-[#58a6ff]" />;
    case 'diff':
      return <GitCommit size={13} className="text-[#3fb950]" />;
    case 'markdown':
      return <FileText size={13} className="text-[#bc8cff]" />;
    case 'terminal':
      return <Terminal size={13} className="text-[#d29922]" />;
    case 'file':
    default:
      return <FileCode size={13} className="text-[#79c0ff]" />;
  }
}

export const ArtifactsLibraryModal = memo(function ArtifactsLibraryModal({
  open,
  onClose,
  onLoadArtifacts,
  onSelectSession,
}: ArtifactsLibraryModalProps) {
  const [items, setItems] = useState<GlobalArtifactItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [category, setCategory] = useState<FilterCategory>('all');
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const modalRef = useModalAccessibility<HTMLDivElement>(open, onClose);

  // Loads all artifacts; returns a canceller so superseded or unmounted
  // requests cannot write stale state.
  const loadArtifacts = useCallback(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    onLoadArtifacts()
      .then((data) => {
        if (cancelled) return;
        setItems(data);
        setSelectedKey(data.length > 0 ? `${data[0].sessionId}:${data[0].artifact.id}` : null);
        setLoading(false);
      })
      .catch((reason) => {
        if (cancelled) return;
        // A failed load must not be indistinguishable from an empty library.
        setError(String(reason));
        setItems([]);
        setSelectedKey(null);
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [onLoadArtifacts]);

  // Load all artifacts on modal open
  useEffect(() => {
    if (!open) return;
    return loadArtifacts();
  }, [open, loadArtifacts]);

  // Filtered artifacts
  const filtered = useMemo(() => {
    const q = (search || '').trim().toLowerCase();
    return items.filter((item) => {
      if (category !== 'all' && item.artifact.type !== category) {
        return false;
      }
      if (!q) return true;
      const titleMatch = item.artifact.title.toLowerCase().includes(q);
      const sessionMatch = item.sessionTitle.toLowerCase().includes(q);
      const pathMatch = (item.artifact.filePath || '').toLowerCase().includes(q);
      const langMatch = (item.artifact.language || '').toLowerCase().includes(q);
      const contentSnippet = item.artifact.content.slice(0, 1000).toLowerCase().includes(q);
      return titleMatch || sessionMatch || pathMatch || langMatch || contentSnippet;
    });
  }, [items, search, category]);

  // Active selected item
  const selectedItem = useMemo(() => {
    if (!selectedKey) return filtered[0] || null;
    return (
      filtered.find((item) => `${item.sessionId}:${item.artifact.id}` === selectedKey) ||
      filtered[0] ||
      null
    );
  }, [filtered, selectedKey]);

  // Handle copy content
  const handleCopy = useCallback(() => {
    if (!selectedItem) return;
    navigator.clipboard.writeText(selectedItem.artifact.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [selectedItem]);

  // Handle navigate to session
  const handleGoToSession = useCallback(() => {
    if (!selectedItem) return;
    onSelectSession(selectedItem.sessionId);
    onClose();
  }, [selectedItem, onSelectSession, onClose]);

  if (!open) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 p-4 backdrop-blur-sm">
      <div
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="artifacts-library-title"
        className="flex h-[88vh] w-[95vw] max-w-6xl flex-col rounded-xl border border-[#30363d] bg-[#0d1117] text-[#e6edf3] shadow-2xl overflow-hidden"
      >
        {/* Header */}
        <div className="flex h-13 items-center justify-between border-b border-[#30363d] px-5 py-3 bg-[#161b22]">
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg border border-[#30363d] bg-[#0d1117]">
              <Layers size={17} className="text-[#58a6ff]" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h2 id="artifacts-library-title" className="text-sm font-semibold text-[#f0f6fc]">
                  Artifacts Library
                </h2>
                <span className="rounded-full border border-[#30363d] bg-[#21262d] px-2 py-0.5 text-[10px] font-medium text-[#8b949e]">
                  {items.length} {items.length === 1 ? 'artifact' : 'artifacts'}
                </span>
              </div>
              <p className="text-[11px] text-[#8b949e]">
                Central searchable vault of all code, files, diffs, and notes across every conversation
              </p>
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close modal"
            className="rounded-md p-1.5 text-[#8b949e] hover:bg-[#21262d] hover:text-[#f0f6fc] transition-colors"
          >
            <X size={18} />
          </button>
        </div>

        {/* Search & Category Filter Controls */}
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#30363d] bg-[#0d1117] px-5 py-2.5">
          <label className="flex min-w-[280px] flex-1 items-center gap-2 rounded-md border border-[#30363d] bg-[#161b22] px-3 py-1.5 focus-within:border-[#58a6ff]">
            <Search size={14} className="text-[#6e7681]" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search by title, path, code snippet, or conversation..."
              className="w-full bg-transparent text-xs text-[#e6edf3] outline-none placeholder:text-[#6e7681]"
            />
            {search && (
              <button
                type="button"
                onClick={() => setSearch('')}
                className="text-[#6e7681] hover:text-[#8b949e]"
                aria-label="Clear search"
              >
                <X size={12} />
              </button>
            )}
          </label>

          {/* Category Chips */}
          <div className="flex items-center gap-1 overflow-x-auto text-xs">
            {(
              [
                { id: 'all', label: 'All' },
                { id: 'code', label: 'Code' },
                { id: 'diff', label: 'Diffs' },
                { id: 'markdown', label: 'Markdown' },
                { id: 'file', label: 'Files' },
                { id: 'terminal', label: 'Terminal' },
              ] as { id: FilterCategory; label: string }[]
            ).map((cat) => (
              <button
                key={cat.id}
                type="button"
                onClick={() => setCategory(cat.id)}
                className={`rounded-md px-2.5 py-1 font-medium transition-colors ${
                  category === cat.id
                    ? 'bg-[#1f6feb] text-white shadow-sm'
                    : 'text-[#8b949e] hover:bg-[#161b22] hover:text-[#e6edf3]'
                }`}
              >
                {cat.label}
              </button>
            ))}
          </div>
        </div>

        {/* Main Split Layout */}
        <div className="flex min-h-0 flex-1 overflow-hidden">
          {/* Left Master List */}
          <div className="flex w-80 shrink-0 flex-col border-r border-[#30363d] bg-[#0d1117]">
            <div className="border-b border-[#30363d] px-3 py-2 text-[10px] font-semibold uppercase tracking-wider text-[#6e7681]">
              Results ({filtered.length})
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto p-2 space-y-1">
              {loading ? (
                <div className="p-8 text-center text-xs text-[#8b949e]">Loading artifacts…</div>
              ) : error ? (
                <div className="p-8 text-center text-xs text-[#f85149]">
                  <p>Could not load artifacts.</p>
                  <p className="mt-1 text-[10px] text-[#6e7681]" title={error}>{error}</p>
                  <button
                    type="button"
                    onClick={loadArtifacts}
                    className="mt-3 rounded-md border border-[#30363d] bg-[#161b22] px-3 py-1.5 text-xs font-medium text-[#e6edf3] transition-colors hover:bg-[#21262d]"
                  >
                    Retry
                  </button>
                </div>
              ) : filtered.length === 0 ? (
                <div className="p-8 text-center text-xs text-[#6e7681]">
                  {items.length === 0 ? 'No artifacts have been generated yet' : 'No matching artifacts found'}
                </div>
              ) : (
                filtered.map((item) => {
                  const itemKey = `${item.sessionId}:${item.artifact.id}`;
                  const isSelected = selectedItem && `${selectedItem.sessionId}:${selectedItem.artifact.id}` === itemKey;
                  return (
                    <button
                      key={itemKey}
                      type="button"
                      onClick={() => setSelectedKey(itemKey)}
                      className={`w-full rounded-md p-2.5 text-left transition-all ${
                        isSelected
                          ? 'border border-[#58a6ff]/40 bg-[#1f6feb]/15 shadow-sm'
                          : 'border border-transparent hover:border-[#30363d] hover:bg-[#161b22]'
                      }`}
                    >
                      <div className="flex items-center gap-2">
                        {getArtifactIcon(item.artifact.type)}
                        <span className="truncate text-xs font-medium text-[#e6edf3]">
                          {item.artifact.title || 'Untitled'}
                        </span>
                      </div>
                      {item.artifact.filePath && (
                        <div className="mt-1 truncate font-mono text-[10px] text-[#58a6ff]">
                          {item.artifact.filePath}
                        </div>
                      )}
                      <div className="mt-1.5 flex items-center justify-between text-[10px] text-[#6e7681]">
                        <span className="truncate max-w-[150px]">{item.sessionTitle}</span>
                        <span>{formatTimestamp(item.artifact.timestamp || item.sessionUpdatedAt)}</span>
                      </div>
                    </button>
                  );
                })
              )}
            </div>
          </div>

          {/* Right Detail / Preview Pane */}
          <div className="flex min-w-0 flex-1 flex-col bg-[#0b0f17]">
            {selectedItem ? (
              <>
                {/* Preview Header */}
                <div className="flex items-center justify-between border-b border-[#30363d] bg-[#161b22] px-5 py-2.5">
                  <div className="min-w-0 flex-1 mr-4">
                    <div className="flex items-center gap-2">
                      {getArtifactIcon(selectedItem.artifact.type)}
                      <h3 className="truncate text-sm font-semibold text-[#f0f6fc]">
                        {selectedItem.artifact.title}
                      </h3>
                      {selectedItem.artifact.language && (
                        <span className="rounded border border-[#30363d] bg-[#0d1117] px-1.5 py-0.5 text-[10px] font-mono text-[#8b949e]">
                          {selectedItem.artifact.language}
                        </span>
                      )}
                    </div>
                    <div className="mt-0.5 flex items-center gap-3 text-[11px] text-[#8b949e]">
                      <span>
                        From chat:{' '}
                        <strong className="font-medium text-[#e6edf3]">{selectedItem.sessionTitle}</strong>
                      </span>
                      <span>•</span>
                      <span>{formatTimestamp(selectedItem.artifact.timestamp || selectedItem.sessionUpdatedAt)}</span>
                    </div>
                  </div>

                  <div className="flex items-center gap-2 shrink-0">
                    <button
                      type="button"
                      onClick={handleCopy}
                      className="flex items-center gap-1.5 rounded-md border border-[#30363d] bg-[#21262d] px-3 py-1.5 text-xs font-medium text-[#e6edf3] hover:bg-[#30363d] transition-colors"
                      title="Copy full content"
                    >
                      {copied ? (
                        <>
                          <Check size={13} className="text-[#3fb950]" />
                          <span>Copied</span>
                        </>
                      ) : (
                        <>
                          <Copy size={13} />
                          <span>Copy</span>
                        </>
                      )}
                    </button>

                    <button
                      type="button"
                      onClick={handleGoToSession}
                      className="flex items-center gap-1.5 rounded-md border border-[#1f6feb]/40 bg-[#1f6feb]/20 px-3 py-1.5 text-xs font-medium text-[#58a6ff] hover:bg-[#1f6feb]/30 transition-colors"
                      title="Open conversation that created this artifact"
                    >
                      <ExternalLink size={13} />
                      <span>Open chat</span>
                    </button>
                  </div>
                </div>

                {/* Preview Content Area */}
                <div className="min-h-0 flex-1 overflow-auto p-4 font-mono text-xs text-[#e6edf3]">
                  {selectedItem.artifact.type === 'diff' ? (
                    <SideBySideDiff diffText={selectedItem.artifact.content} />
                  ) : selectedItem.artifact.type === 'markdown' ? (
                    <div className="rounded-lg border border-[#30363d] bg-[#161b22] p-4 font-sans text-sm">
                      <MarkdownRenderer content={selectedItem.artifact.content} />
                    </div>
                  ) : (
                    <div className="rounded-lg border border-[#30363d] bg-[#161b22] p-4 overflow-x-auto">
                      <pre className="whitespace-pre leading-relaxed">
                        <code>{selectedItem.artifact.content}</code>
                      </pre>
                    </div>
                  )}
                </div>
              </>
            ) : (
              <div className="flex h-full flex-col items-center justify-center p-8 text-center text-[#6e7681]">
                <Archive size={32} className="mb-2 opacity-50" />
                <p className="text-sm">Select an artifact from the list to preview</p>
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
});
