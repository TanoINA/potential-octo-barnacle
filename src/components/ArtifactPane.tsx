import { useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Check, Code, Columns2, Copy, Eye, FileCode2, GitCompare, Rows, Terminal, TerminalSquare, X } from 'lucide-react';
import type { Artifact } from '../types/agent';
import { SideBySideDiff } from './SideBySideDiff';
import { InteractiveTerminal } from './InteractiveTerminal';
import { MarkdownRenderer } from './MarkdownRenderer';

type ViewerTab = 'code' | 'diff' | 'terminal' | 'shell';

interface ArtifactPaneProps {
  artifacts: Artifact[];
  activeArtifactId: string | null;
  onSelectArtifact: (id: string) => void;
  onClose: () => void;
  initialTab?: ViewerTab;
  overlay?: boolean;
  width?: number;
}

function matchesTab(artifact: Artifact, tab: ViewerTab): boolean {
  if (tab === 'shell') return false;
  if (tab === 'diff') return artifact.type === 'diff';
  if (tab === 'terminal') return artifact.type === 'terminal';
  return artifact.type !== 'diff' && artifact.type !== 'terminal';
}

function svgPreviewDoc(svg: string): string {
  return `<!DOCTYPE html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:;"><style>html,body{margin:0;padding:16px;min-height:100%;box-sizing:border-box;display:flex;align-items:center;justify-content:center;background:#0d1117;}svg{max-width:100%;max-height:100%;}</style></head><body>${svg}</body></html>`;
}

function fencedCodeMarkdown(language: string, content: string): string {
  let maxRun = 0;
  for (const match of content.matchAll(/`+/g)) {
    if (match[0].length > maxRun) maxRun = match[0].length;
  }
  const fence = '`'.repeat(Math.max(3, maxRun + 1));
  const lang = language.replace(/`/g, '').trim();
  return `${fence}${lang}\n${content}\n${fence}`;
}

export function ArtifactPane({ artifacts, activeArtifactId, onSelectArtifact, onClose, initialTab, overlay = false, width }: ArtifactPaneProps) {
  const initial = artifacts.find((artifact) => artifact.id === activeArtifactId) || artifacts[0];
  const [tab, setTab] = useState<ViewerTab>(initialTab || (artifacts.length === 0 ? 'shell' : (initial?.type === 'diff' || initial?.type === 'terminal' ? initial.type : 'code')));
  const [diffViewMode, setDiffViewMode] = useState<'unified' | 'side-by-side'>('side-by-side');
  const [codeViewMode, setCodeViewMode] = useState<'preview' | 'raw'>('preview');
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
    <aside
      style={!overlay && width ? { width: `${width}px`, maxWidth: '55vw' } : undefined}
      className={overlay
        ? 'ui-drawer-enter fixed inset-y-0 right-0 z-40 flex h-full w-[min(90vw,420px)] min-w-0 flex-col border-l border-[var(--stroke-nous)] bg-[var(--ui-bg-chrome)] text-[var(--ui-text-primary)] shadow-lg'
        : 'ui-drawer-enter relative flex h-full min-w-[280px] shrink-0 flex-col border-l border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-chrome)] text-[var(--ui-text-primary)]'
      }
    >
      <header className="flex h-10 shrink-0 items-center gap-2 border-b border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-sidebar)] px-3">
        <span className="min-w-0 flex-1 truncate text-xs font-semibold text-[var(--ui-text-primary)]">
          {tab === 'shell' ? 'Live Shell' : active?.title || (tab === 'diff' ? 'Diff Viewer' : tab === 'terminal' ? 'Tool Output' : 'Code Viewer')}
        </span>
        {active && (
          <button type="button" onClick={() => void copy()} className="icon-button" aria-label="Copy artifact content" title="Copy content">
            {copied ? <Check size={15} className="text-[var(--ui-green)]" /> : <Copy size={15} />}
          </button>
        )}
        <button type="button" onClick={onClose} className="icon-button" aria-label="Close artifact sidecar">
          <X size={16} />
        </button>
      </header>
      <div className="flex shrink-0 overflow-x-auto border-b border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-sidebar)] px-2 pt-1">
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
              className={`flex shrink-0 items-center gap-1.5 border-b-2 px-2.5 py-2 text-[10.5px] transition-colors duration-100 hover:transition-none disabled:cursor-not-allowed disabled:opacity-30 ${
                tab === item.id ? 'border-[var(--ui-accent)] text-[var(--ui-accent)] font-medium' : 'border-transparent text-[var(--ui-text-tertiary)] hover:text-[var(--ui-text-primary)]'
              }`}
            >
              <Icon size={12} /> {item.label}
            </button>
          );
        })}
      </div>
      {tab !== 'shell' && active && (
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--ui-stroke-quaternary)] bg-[var(--ui-bg-chrome)] px-3 py-2">
          <select
            value={active.id}
            onChange={(event) => onSelectArtifact(event.target.value)}
            className="h-7 min-w-0 flex-1 rounded-md border border-[var(--ui-stroke-secondary)] bg-[var(--ui-bg-editor)] px-2 text-[10.5px] text-[var(--ui-text-primary)] outline-none"
            aria-label="Select artifact"
          >
            {tabArtifacts.map((artifact) => (
              <option key={artifact.id} value={artifact.id}>
                {artifact.title}
              </option>
            ))}
          </select>
          {tab === 'diff' && (
            <div className="flex items-center rounded-md bg-[var(--ui-control-fill)] p-0.5 text-[10px]">
              <button
                type="button"
                onClick={() => setDiffViewMode('unified')}
                className={`flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-100 hover:transition-none ${
                  diffViewMode === 'unified' ? 'bg-[var(--ui-bg-elevated)] text-[var(--ui-accent)] font-medium' : 'text-[var(--ui-text-tertiary)] hover:text-[var(--ui-text-primary)]'
                }`}
                title="Unified single-column diff"
              >
                <Rows size={11} />
                <span>Unified</span>
              </button>
              <button
                type="button"
                onClick={() => setDiffViewMode('side-by-side')}
                className={`flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-100 hover:transition-none ${
                  diffViewMode === 'side-by-side' ? 'bg-[var(--ui-bg-elevated)] text-[var(--ui-accent)] font-medium' : 'text-[var(--ui-text-tertiary)] hover:text-[var(--ui-text-primary)]'
                }`}
                title="Side-by-side split visual diff"
              >
                <Columns2 size={11} />
                <span>Split</span>
              </button>
            </div>
          )}
          {tab === 'code' && (
            <div className="flex items-center rounded-md bg-[var(--ui-control-fill)] p-0.5 text-[10px]">
              <button
                type="button"
                onClick={() => setCodeViewMode('preview')}
                className={`flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-100 hover:transition-none ${
                  codeViewMode === 'preview' ? 'bg-[var(--ui-bg-elevated)] text-[#58a6ff] font-medium' : 'text-[var(--ui-text-tertiary)] hover:text-[var(--ui-text-primary)]'
                }`}
                title="Rendered preview"
                aria-label="Preview"
              >
                <Eye size={11} />
                <span>Preview</span>
              </button>
              <button
                type="button"
                onClick={() => setCodeViewMode('raw')}
                className={`flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-100 hover:transition-none ${
                  codeViewMode === 'raw' ? 'bg-[var(--ui-bg-elevated)] text-[#58a6ff] font-medium' : 'text-[var(--ui-text-tertiary)] hover:text-[var(--ui-text-primary)]'
                }`}
                title="Raw source"
                aria-label="Raw"
              >
                <Code size={11} />
                <span>Raw</span>
              </button>
            </div>
          )}
          <span className="shrink-0 font-mono text-[9.5px] text-[var(--ui-text-quaternary)]">{lines.length} lines</span>
        </div>
      )}
      {tab !== 'shell' && active?.filePath && (
        <div className="shrink-0 truncate border-b border-[var(--ui-stroke-tertiary)] px-3 py-1.5 font-mono text-[9px] text-[var(--ui-text-quaternary)]" title={active.filePath}>
          {active.filePath}
        </div>
      )}
      {tab === 'shell' ? (
        <InteractiveTerminal />
      ) : tab === 'diff' && diffViewMode === 'side-by-side' && active ? (
        <SideBySideDiff diffText={active.content} />
      ) : tab === 'code' && codeViewMode === 'preview' && active ? (
        (() => {
          const contentTrimmed = (active.content || '').trim();
          const isHtml =
            active.language === 'html' ||
            active.filePath?.endsWith('.html') ||
            contentTrimmed.startsWith('<!DOCTYPE html') ||
            contentTrimmed.startsWith('<html');
          const isSvg =
            active.language === 'svg' ||
            active.filePath?.endsWith('.svg') ||
            contentTrimmed.startsWith('<svg');

          if (isHtml) {
            return (
              <div className="h-full w-full bg-white">
                <iframe
                  title={active.title || 'HTML preview'}
                  srcDoc={active.content}
                  className="h-full w-full border-none"
                  sandbox="allow-scripts"
                />
              </div>
            );
          }

          if (isSvg) {
            return (
              <div className="h-full w-full bg-[#0d1117]">
                <iframe
                  title={active.title || 'SVG preview'}
                  srcDoc={svgPreviewDoc(active.content)}
                  className="h-full w-full border-none bg-[#0d1117]"
                  sandbox=""
                />
              </div>
            );
          }

          return (
            <div className="min-h-0 flex-1 select-text overflow-auto bg-[var(--ui-bg-chrome)] p-4 text-xs leading-relaxed text-[var(--ui-text-primary)]">
              {active.type === 'markdown' || active.language === 'markdown' || active.filePath?.endsWith('.md') || !active.language ? (
                <MarkdownRenderer content={active.content} />
              ) : (
                <MarkdownRenderer content={fencedCodeMarkdown(active.language, active.content)} />
              )}
            </div>
          );
        })()
      ) : active ? (
        <div
          ref={scrollRef}
          onScroll={(event) => {
            if (tab !== 'terminal') return;
            const element = event.currentTarget;
            stickTerminalToBottomRef.current = element.scrollHeight - element.scrollTop - element.clientHeight < 40;
          }}
          className="min-h-0 flex-1 select-text overflow-auto bg-[var(--ui-bg-chrome)] py-2 font-mono text-[11px] leading-5"
        >
          <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((virtualLine) => {
              const line = lines[virtualLine.index] || '';
              const diffClass =
                tab === 'diff'
                  ? line.startsWith('+') && !line.startsWith('+++')
                    ? 'bg-[var(--ui-diff-add-bg)] text-[var(--ui-diff-add-fg)]'
                    : line.startsWith('-') && !line.startsWith('---')
                    ? 'bg-[var(--ui-diff-remove-bg)] text-[var(--ui-diff-remove-fg)]'
                    : line.startsWith('@@')
                    ? 'text-[var(--ui-accent)]'
                    : 'text-[var(--ui-text-tertiary)]'
                  : 'text-[var(--ui-text-secondary)]';
              return (
                <div
                  key={virtualLine.key}
                  data-index={virtualLine.index}
                  ref={virtualizer.measureElement}
                  className={`absolute left-0 top-0 flex min-h-5 w-full whitespace-pre-wrap break-words ${diffClass}`}
                  style={{ transform: `translateY(${virtualLine.start}px)` }}
                >
                  <span className="w-11 shrink-0 select-none pr-2 text-right text-[var(--ui-text-quaternary)]">{virtualLine.index + 1}</span>
                  <span className="min-w-0 flex-1 pr-3">{line || ' '}</span>
                </div>
              );
            })}
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col items-center justify-center p-6 text-center text-[var(--ui-text-tertiary)]">
          <FileCode2 size={32} className="mb-2 text-[var(--ui-text-quaternary)]" />
          <span className="text-xs font-semibold text-[var(--ui-text-primary)]">No {tab} artifacts yet</span>
          <span className="mt-1 max-w-xs text-[11px] leading-relaxed">
            Tool executions, file edits, and diffs generated by AetherAgent will appear here.
          </span>
          <button
            type="button"
            onClick={() => setTab('shell')}
            className="mt-4 flex items-center gap-1.5 rounded-lg border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] px-3 py-1.5 text-xs font-medium text-[var(--ui-accent)] hover:bg-[var(--ui-control-hover)]"
          >
            <Terminal size={13} />
            <span>Open Live Shell</span>
          </button>
        </div>
      )}
    </aside>
  );
}
