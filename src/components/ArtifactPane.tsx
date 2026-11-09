import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Check, Columns2, Copy, FileCode2, GitCompare, Rows, Terminal, TerminalSquare, X } from 'lucide-react';
import type { Artifact } from '../types/agent';
import { SideBySideDiff } from './SideBySideDiff';
import { InteractiveTerminal } from './InteractiveTerminal';

type ViewerTab = 'code' | 'diff' | 'terminal' | 'shell';

interface ArtifactPaneProps {
  artifacts: Artifact[];
  activeArtifactId: string | null;
  onSelectArtifact: (id: string) => void;
  onClose: () => void;
  initialTab?: ViewerTab;
  overlay?: boolean;
}

function matchesTab(artifact: Artifact, tab: ViewerTab): boolean {
  if (tab === 'shell') return false;
  if (tab === 'diff') return artifact.type === 'diff';
  if (tab === 'terminal') return artifact.type === 'terminal';
  return artifact.type !== 'diff' && artifact.type !== 'terminal';
}

export function ArtifactPane({ artifacts, activeArtifactId, onSelectArtifact, onClose, initialTab, overlay = false }: ArtifactPaneProps) {
  const initial = artifacts.find((artifact) => artifact.id === activeArtifactId) || artifacts[0];
  const [tab, setTab] = useState<ViewerTab>(initialTab || (artifacts.length === 0 ? 'shell' : (initial?.type === 'diff' || initial?.type === 'terminal' ? initial.type : 'code')));
  const [diffViewMode, setDiffViewMode] = useState<'unified' | 'side-by-side'>('side-by-side');
  const [copied, setCopied] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const previousActiveIdRef = useRef(activeArtifactId);
  const stickTerminalToBottomRef = useRef(true);
  const copyTimerRef = useRef<number | null>(null);

  useEffect(() => {
    if (initialTab) {
      setTab(initialTab);
    }
  }, [initialTab]);

  useEffect(() => {
    if (activeArtifactId === previousActiveIdRef.current) return;
    const active = artifacts.find((artifact) => artifact.id === activeArtifactId);
    if (active) {
      previousActiveIdRef.current = activeArtifactId;
      stickTerminalToBottomRef.current = true;
      // Artifact arrival must not unmount a user-active shell and lose its state.
      setTab((current) => current === 'shell' ? current : active.type === 'diff' || active.type === 'terminal' ? active.type : 'code');
    }
  }, [activeArtifactId, artifacts]);

  const tabArtifacts = useMemo(() => artifacts.filter((artifact) => matchesTab(artifact, tab)), [artifacts, tab]);
  const active = tabArtifacts.find((artifact) => artifact.id === activeArtifactId) || tabArtifacts[0] || null;
  const lines = useMemo(() => active?.content.split('\n') || [], [active?.content]);
  const virtualizer = useVirtualizer({
    count: lines.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 20,
    overscan: 10,
  });

  useEffect(() => {
    setCopied(false);
    stickTerminalToBottomRef.current = true;
    virtualizer.scrollToIndex(0);
  }, [active?.id]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => () => {
    if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
  }, []);

  useEffect(() => {
    if (tab === 'terminal' && stickTerminalToBottomRef.current && lines.length > 0) virtualizer.scrollToIndex(lines.length - 1, { align: 'end' });
  }, [active?.content, lines.length, tab]); // eslint-disable-line react-hooks/exhaustive-deps

  const selectTab = (next: ViewerTab) => {
    setTab(next);
    const artifact = artifacts.find((item) => matchesTab(item, next));
    if (artifact) onSelectArtifact(artifact.id);
  };

  const copy = async () => {
    if (!active) return;
    try {
      await navigator.clipboard.writeText(active.content);
      setCopied(true);
      if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
      copyTimerRef.current = window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };

  const tabs: { id: ViewerTab; label: string; icon: typeof FileCode2 }[] = [
    { id: 'code', label: 'Code Viewer', icon: FileCode2 },
    { id: 'diff', label: 'Diff Viewer', icon: GitCompare },
    { id: 'terminal', label: 'Tool Output', icon: TerminalSquare },
    { id: 'shell', label: 'Live Shell', icon: Terminal },
  ];

  return (
    <aside className={overlay
      ? 'ui-drawer-enter fixed inset-y-0 right-0 z-40 flex h-full w-[min(90vw,420px)] min-w-0 flex-col border-l border-[#30363d] bg-[#0d1117] text-[#e6edf3] shadow-lg'
      : 'ui-drawer-enter relative flex h-full w-[min(38vw,420px)] min-w-[260px] max-w-[460px] shrink-0 flex-col border-l border-[#30363d] bg-[#0d1117] text-[#e6edf3]'}>
      <header className="flex h-11 shrink-0 items-center gap-2 border-b border-[#30363d] bg-[#161b22] px-3">
        <span className="min-w-0 flex-1 truncate text-xs font-semibold">
          {tab === 'shell' ? 'Live Shell' : active?.title || (tab === 'diff' ? 'Diff Viewer' : tab === 'terminal' ? 'Tool Output' : 'Code Viewer')}
        </span>
        {active && (
          <button type="button" onClick={() => void copy()} className="icon-button" aria-label="Copy artifact content" title="Copy content">
            {copied ? <Check size={15} className="text-[#3fb950]" /> : <Copy size={15} />}
          </button>
        )}
        <button type="button" onClick={onClose} className="icon-button" aria-label="Close artifact sidecar">
          <X size={16} />
        </button>
      </header>
      <div className="flex shrink-0 overflow-x-auto border-b border-[#30363d] px-2 pt-2">
        {tabs.map((item) => {
          const enabled = item.id === 'shell' || artifacts.some((artifact) => matchesTab(artifact, item.id));
          const Icon = item.icon;
          return (
            <button
              key={item.id}
              type="button"
              disabled={!enabled}
              title={item.label}
              onClick={() => selectTab(item.id)}
              className={`flex shrink-0 items-center gap-1.5 border-b-2 px-2.5 py-2 text-[10px] disabled:cursor-not-allowed disabled:opacity-30 ${
                tab === item.id ? 'border-[#58a6ff] text-[#58a6ff]' : 'border-transparent text-[#8b949e] hover:text-[#e6edf3]'
              }`}
            >
              <Icon size={12} /> {item.label}
            </button>
          );
        })}
      </div>
      {tab !== 'shell' && active && (
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[#30363d] bg-[#0d1117] px-3 py-2">
          <select
            value={active.id}
            onChange={(event) => onSelectArtifact(event.target.value)}
            className="h-7 min-w-0 flex-1 rounded border border-[#30363d] bg-[#161b22] px-2 text-[10px] outline-none"
            aria-label="Select artifact"
          >
            {tabArtifacts.map((artifact) => (
              <option key={artifact.id} value={artifact.id}>
                {artifact.title}
              </option>
            ))}
          </select>
          {tab === 'diff' && (
            <div className="flex items-center rounded border border-[#30363d] bg-[#161b22] p-0.5 text-[10px]">
              <button
                type="button"
                onClick={() => setDiffViewMode('unified')}
                className={`flex items-center gap-1 rounded px-1.5 py-0.5 ${
                  diffViewMode === 'unified' ? 'bg-[#21262d] text-[#58a6ff] font-medium' : 'text-[#8b949e] hover:text-white'
                }`}
                title="Unified single-column diff"
              >
                <Rows size={11} />
                <span>Unified</span>
              </button>
              <button
                type="button"
                onClick={() => setDiffViewMode('side-by-side')}
                className={`flex items-center gap-1 rounded px-1.5 py-0.5 ${
                  diffViewMode === 'side-by-side' ? 'bg-[#21262d] text-[#58a6ff] font-medium' : 'text-[#8b949e] hover:text-white'
                }`}
                title="Side-by-side split visual diff"
              >
                <Columns2 size={11} />
                <span>Split</span>
              </button>
            </div>
          )}
          <span className="shrink-0 text-[9px] text-[#6e7681]">{lines.length} lines</span>
        </div>
      )}
      {tab !== 'shell' && active?.filePath && (
        <div className="shrink-0 truncate border-b border-[#30363d] px-3 py-1.5 font-mono text-[9px] text-[#6e7681]" title={active.filePath}>
          {active.filePath}
        </div>
      )}
      {tab === 'shell' ? (
        <InteractiveTerminal />
      ) : tab === 'diff' && diffViewMode === 'side-by-side' && active ? (
        <SideBySideDiff diffText={active.content} />
      ) : active ? (
        <div
          ref={scrollRef}
          onScroll={(event) => {
            if (tab !== 'terminal') return;
            const element = event.currentTarget;
            stickTerminalToBottomRef.current = element.scrollHeight - element.scrollTop - element.clientHeight < 40;
          }}
          className="min-h-0 flex-1 select-text overflow-auto bg-[#0d1117] py-2 font-mono text-[11px] leading-5"
        >
          <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((virtualLine) => {
              const line = lines[virtualLine.index] || '';
              const diffClass =
                tab === 'diff'
                  ? line.startsWith('+') && !line.startsWith('+++')
                    ? 'bg-[#3fb950]/10 text-[#7ee787]'
                    : line.startsWith('-') && !line.startsWith('---')
                    ? 'bg-[#f85149]/10 text-[#ff7b72]'
                    : line.startsWith('@@')
                    ? 'text-[#79c0ff]'
                    : 'text-[#8b949e]'
                  : 'text-[#c9d1d9]';
              return (
                <div
                  key={virtualLine.key}
                  data-index={virtualLine.index}
                  ref={virtualizer.measureElement}
                  className={`absolute left-0 top-0 flex min-h-5 w-full whitespace-pre-wrap break-words ${diffClass}`}
                  style={{ transform: `translateY(${virtualLine.start}px)` }}
                >
                  <span className="w-11 shrink-0 select-none pr-2 text-right text-[#484f58]">{virtualLine.index + 1}</span>
                  <span className="min-w-0 flex-1 pr-3">{line || ' '}</span>
                </div>
              );
            })}
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col items-center justify-center p-6 text-center text-[#8b949e]">
          <FileCode2 size={32} className="mb-2 text-[#30363d]" />
          <span className="text-xs font-semibold text-[#e6edf3]">No {tab} artifacts yet</span>
          <span className="mt-1 max-w-xs text-[11px] leading-relaxed">
            Tool executions, file edits, and diffs generated by AetherAgent will appear here.
          </span>
          <button
            type="button"
            onClick={() => setTab('shell')}
            className="mt-4 flex items-center gap-1.5 rounded-lg border border-[#30363d] bg-[#161b22] px-3 py-1.5 text-xs font-medium text-[#58a6ff] hover:bg-[#21262d]"
          >
            <Terminal size={13} />
            <span>Open Live Shell</span>
          </button>
        </div>
      )}
    </aside>
  );
}
