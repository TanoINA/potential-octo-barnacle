import { memo, useEffect, useMemo, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { Check, Download, Folder, FolderPlus, Layers, MessageSquare, MoreHorizontal, Pencil, Pin, Plus, Search, Settings, Trash2, X } from 'lucide-react';
import type { DeepSearchResult, EffectiveRunConfig, Session } from '../types/agent';
import { AetherLogo } from './AetherLogo';

interface SidebarProps {
  sessions: Session[];
  currentSessionId: string;
  onNewSession: () => void;
  onSelectSession: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onRenameSession: (id: string, title: string) => void;
  onTogglePinSession?: (id: string) => void;
  onDeepSearch?: (query: string) => Promise<DeepSearchResult[]>;
  onExportSession: (id: string, format: 'json' | 'markdown' | 'html') => void;
  onOpenSettings: () => void;
  onOpenArtifactsLibrary?: () => void;
  workspaceCwd: string;
  onSelectWorkspace: () => void;
  onClearWorkspace?: () => void;
  effectiveConfig: EffectiveRunConfig | null;
  overlay?: boolean;
}

const SNIPPET_HIGHLIGHT_CLASS = 'bg-[#d29922]/30 text-[#f0883e] font-semibold px-0.5 rounded';

/**
 * Renders a deep-search snippet safely. The backend (SQLite FTS `snippet()`)
 * wraps matches in <mark>…</mark>, but every other byte of the snippet is
 * untrusted stored content (messages, artifacts). It is therefore rendered as
 * literal text — never parsed as HTML — and only the <mark> markers become
 * highlight elements. This blocks stored XSS through search results.
 */
function renderSnippetWithHighlights(snippet: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const marker = /<mark>|<\/mark>/g;
  let lastIndex = 0;
  let inHighlight = false;
  let key = 0;
  let match: RegExpExecArray | null;
  while ((match = marker.exec(snippet)) !== null) {
    const text = snippet.slice(lastIndex, match.index);
    if (text) {
      nodes.push(
        inHighlight
          ? <mark key={`snippet-hl-${key++}`} className={SNIPPET_HIGHLIGHT_CLASS}>{text}</mark>
          : text,
      );
    }
    inHighlight = match[0] === '<mark>';
    lastIndex = marker.lastIndex;
  }
  const tail = snippet.slice(lastIndex);
  if (tail) {
    nodes.push(
      inHighlight
        ? <mark key={`snippet-hl-${key++}`} className={SNIPPET_HIGHLIGHT_CLASS}>{tail}</mark>
        : tail,
    );
  }
  return nodes;
}

export const Sidebar = memo(function Sidebar({
  sessions,
  currentSessionId,
  onNewSession,
  onSelectSession,
  onDeleteSession,
  onRenameSession,
  onTogglePinSession,
  onDeepSearch,
  onExportSession,
  onOpenSettings,
  onOpenArtifactsLibrary,
  workspaceCwd,
  onSelectWorkspace,
  onClearWorkspace,
  effectiveConfig,
  overlay = false,
}: SidebarProps) {
  const [search, setSearch] = useState('');
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [menuId, setMenuId] = useState<string | null>(null);
  const [menuPosition, setMenuPosition] = useState({ top: 0, left: 0 });
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [deepResults, setDeepResults] = useState<Record<string, DeepSearchResult>>({});
  const [isDeepSearching, setIsDeepSearching] = useState(false);

  useEffect(() => {
    if (!menuId) return;
    const onMouseDown = (event: MouseEvent) => {
      const target = event.target as HTMLElement;
      if (!target.closest('[data-session-menu]') && !target.closest('[data-session-menu-trigger]')) {
        setMenuId(null);
      }
    };
    const onKeyDown = (event: KeyboardEvent) => { if (event.key === 'Escape') setMenuId(null); };
    const closeOnScroll = () => setMenuId(null);
    window.addEventListener('mousedown', onMouseDown);
    window.addEventListener('keydown', onKeyDown);
    window.addEventListener('scroll', closeOnScroll, true);
    return () => {
      window.removeEventListener('mousedown', onMouseDown);
      window.removeEventListener('keydown', onKeyDown);
      window.removeEventListener('scroll', closeOnScroll, true);
    };
  }, [menuId]);

  // Deep Full-Text Search effect
  useEffect(() => {
    const q = search.trim();
    if (q.length < 2 || !onDeepSearch) {
      setDeepResults({});
      setIsDeepSearching(false);
      return;
    }
    let cancelled = false;
    setIsDeepSearching(true);
    const timer = setTimeout(() => {
      onDeepSearch(q)
        .then((results) => {
          if (!cancelled) {
            const map: Record<string, DeepSearchResult> = {};
            for (const r of results) {
              map[r.sessionId] = r;
            }
            setDeepResults(map);
            setIsDeepSearching(false);
          }
        })
        .catch(() => {
          if (!cancelled) setIsDeepSearching(false);
        });
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [search, onDeepSearch]);

  const filtered = useMemo(() => {
    const query = search.trim().toLowerCase();
    if (!query) return sessions;
    return sessions.filter((session) => {
      const titleMatch = `${session.title} ${session.preview || ''}`.toLowerCase().includes(query);
      const deepMatch = Boolean(deepResults[session.id]);
      return titleMatch || deepMatch;
    });
  }, [sessions, search, deepResults]);

  const pinnedSessions = useMemo(() => filtered.filter((s) => Boolean(s.pinned)), [filtered]);
  const recentSessions = useMemo(() => filtered.filter((s) => !s.pinned), [filtered]);

  const commitRename = (id: string) => {
    if (renameValue.trim()) onRenameSession(id, renameValue.trim());
    setRenamingId(null);
  };

  const renderSessionItem = (session: Session) => {
    const active = session.id === currentSessionId;
    const renaming = renamingId === session.id;
    const deepResult = deepResults[session.id];

    return (
      <div key={session.id} className={`group relative mb-0.5 rounded-md border ${active ? 'border-[#58a6ff]/35 bg-[#1f6feb]/15' : 'border-transparent hover:bg-[#161b22]'}`}>
        {renaming ? (
          <div className="flex items-center gap-1 p-1.5">
            <input
              autoFocus
              value={renameValue}
              onChange={(event) => setRenameValue(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') commitRename(session.id);
                if (event.key === 'Escape') setRenamingId(null);
              }}
              className="min-w-0 flex-1 rounded border border-[#58a6ff] bg-[#0d1117] px-2 py-1 text-xs outline-none"
              aria-label="Session title"
            />
            <button type="button" className="icon-button" onClick={() => commitRename(session.id)} aria-label="Save title"><Check size={13} /></button>
          </div>
        ) : deleteId === session.id ? (
          <div className="flex items-center gap-1.5 px-2.5 py-2 text-[11px] text-[#f85149]">
            <span className="flex-1">Delete this chat?</span>
            <button type="button" className="rounded bg-[#da3633] px-2 py-1 text-white" onClick={() => { onDeleteSession(session.id); setDeleteId(null); }}>Delete</button>
            <button type="button" className="text-[#8b949e]" onClick={() => setDeleteId(null)}>Cancel</button>
          </div>
        ) : (
          <div className="flex items-center">
            <button
              type="button"
              onClick={(e) => {
                if (e.shiftKey && onTogglePinSession) {
                  e.preventDefault();
                  onTogglePinSession(session.id);
                  return;
                }
                onSelectSession(session.id);
              }}
              className="min-w-0 flex-1 px-2.5 py-2 text-left"
              title={session.pinned ? 'Pinned chat (Shift+Click to unpin)' : 'Shift+Click to pin chat to top'}
            >
              <span className="flex items-center gap-2">
                {session.pinned ? (
                  <Pin size={12} className="shrink-0 text-[#d29922]" />
                ) : (
                  <MessageSquare size={13} className={`shrink-0 ${active ? 'text-[#58a6ff]' : 'text-[#6e7681]'}`} />
                )}
                <span className="truncate text-xs">{session.title || 'Untitled'}</span>
              </span>
              {session.preview && !deepResult && (
                <span className="ml-5 mt-0.5 block truncate text-[10px] text-[#6e7681]">{session.preview}</span>
              )}
              {deepResult?.snippet && (
                <div
                  className="ml-5 mt-1 rounded border border-[#30363d]/60 bg-[#0d1117] px-2 py-1 font-mono text-[10px] text-[#8b949e] line-clamp-2 leading-tight"
                >
                  {renderSnippetWithHighlights(deepResult.snippet)}
                </div>
              )}
            </button>
            <button
              type="button"
              data-session-menu-trigger
              className="icon-button mr-1 opacity-0 focus:opacity-100 group-hover:opacity-100"
              onClick={(event) => {
                if (menuId === session.id) {
                  setMenuId(null);
                  return;
                }
                const rect = event.currentTarget.getBoundingClientRect();
                const width = 160;
                const estimatedHeight = 220;
                setMenuPosition({
                  left: Math.max(8, Math.min(window.innerWidth - width - 8, rect.right - width)),
                  top: rect.bottom + estimatedHeight > window.innerHeight
                    ? Math.max(8, rect.top - estimatedHeight)
                    : rect.bottom + 4,
                });
                setMenuId(session.id);
              }}
              aria-label={`Actions for ${session.title}`}
              aria-expanded={menuId === session.id}
            ><MoreHorizontal size={14} /></button>
          </div>
        )}
        {menuId === session.id && createPortal(
          <div data-session-menu className="fixed z-[60] w-44 rounded-md border border-[#30363d] bg-[#161b22] p-1 shadow-2xl" style={menuPosition}>
            {onTogglePinSession && (
              <button
                type="button"
                className="session-action"
                onClick={() => {
                  onTogglePinSession(session.id);
                  setMenuId(null);
                }}
              >
                <Pin size={12} className={session.pinned ? 'text-[#d29922]' : ''} />
                {session.pinned ? 'Unpin chat' : 'Pin chat to top'}
              </button>
            )}
            <button type="button" className="session-action" onClick={() => { setRenamingId(session.id); setRenameValue(session.title); setMenuId(null); }}><Pencil size={12} /> Rename</button>
            <button type="button" className="session-action" onClick={() => { onExportSession(session.id, 'html'); setMenuId(null); }}><Download size={12} /> Export HTML (.html)</button>
            <button type="button" className="session-action" onClick={() => { onExportSession(session.id, 'markdown'); setMenuId(null); }}><Download size={12} /> Export Markdown</button>
            <button type="button" className="session-action" onClick={() => { onExportSession(session.id, 'json'); setMenuId(null); }}><Download size={12} /> Export JSON</button>
            <button type="button" className="session-action text-[#f85149]" onClick={() => { setDeleteId(session.id); setMenuId(null); }}><Trash2 size={12} /> Delete</button>
          </div>,
          document.body,
        )}
      </div>
    );
  };

  return (
    <aside className={`${overlay ? 'fixed inset-y-0 left-0 z-40 shadow-2xl' : 'relative'} flex h-full w-64 shrink-0 flex-col border-r border-[#30363d] bg-[#0d1117] text-[#e6edf3]`}>
      <div className="flex h-11 items-center justify-between border-b border-[#30363d] px-3">
        <div className="flex items-center gap-2.5">
          <div className="flex h-7 w-7 items-center justify-center rounded-lg border border-[#30363d] bg-[#161b22]">
            <AetherLogo size={15} />
          </div>
          <span className="text-xs font-semibold tracking-wide text-[#e6edf3]">AetherAgent</span>
        </div>
        <button type="button" onClick={onNewSession} className="icon-button" aria-label="New chat" title="New chat (Ctrl+N)"><Plus size={16} /></button>
      </div>

      <div className="space-y-2 p-3">
        <div className="flex items-center gap-1.5">
          <button type="button" onClick={onNewSession} className="flex min-w-0 flex-1 items-center justify-center gap-2 rounded-md border border-[#30363d] bg-[#161b22] py-2 text-xs font-medium hover:bg-[#21262d] transition-colors"><Plus size={14} /> New chat</button>
          {onOpenArtifactsLibrary && (
            <button
              type="button"
              onClick={onOpenArtifactsLibrary}
              className="flex items-center justify-center rounded-md border border-[#30363d] bg-[#161b22] p-2 text-xs text-[#8b949e] hover:bg-[#21262d] hover:text-[#58a6ff] transition-colors"
              title="Artifacts Library"
              aria-label="Open global artifacts library"
            >
              <Layers size={14} />
            </button>
          )}
        </div>
        <label className="flex items-center gap-2 rounded-md border border-[#30363d] bg-[#161b22] px-2.5 focus-within:border-[#58a6ff]">
          <Search size={13} className={isDeepSearching ? 'text-[#58a6ff] animate-pulse' : 'text-[#6e7681]'} />
          <input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder="Search messages & artifacts"
            className="h-8 min-w-0 flex-1 bg-transparent text-xs outline-none placeholder:text-[#6e7681]"
          />
          {search && <button type="button" onClick={() => setSearch('')} aria-label="Clear search"><X size={12} className="text-[#8b949e]" /></button>}
        </label>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {filtered.length === 0 ? (
          <div className="px-3 py-8 text-center text-xs text-[#6e7681]">{search ? 'No matching conversations' : 'No saved conversations yet'}</div>
        ) : (
          <>
            {pinnedSessions.length > 0 && (
              <div className="mb-3">
                <div className="flex items-center justify-between px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-[#d29922]">
                  <span className="flex items-center gap-1.5">
                    <Pin size={11} className="text-[#d29922]" />
                    <span>Pinned ({pinnedSessions.length})</span>
                  </span>
                  <span className="text-[9px] font-normal normal-case text-[#8b949e]">Shift-click to toggle</span>
                </div>
                {pinnedSessions.map(renderSessionItem)}
              </div>
            )}

            <div>
              <div className="flex items-center justify-between px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-[#6e7681]">
                <span>Recent conversations ({recentSessions.length})</span>
              </div>
              {recentSessions.length === 0 && pinnedSessions.length > 0 && (
                <div className="px-3 py-3 text-center text-[11px] text-[#6e7681]">All matching chats are pinned above</div>
              )}
              {recentSessions.map(renderSessionItem)}
            </div>
          </>
        )}
      </div>

      <div className="border-t border-[#30363d] p-3">
        {workspaceCwd ? (
          <div className="mb-2 flex w-full items-center gap-1.5 rounded-md border border-[#30363d] bg-[#161b22] px-2.5 py-1.5 text-left group">
            <button
              type="button"
              onClick={onSelectWorkspace}
              className="flex items-center gap-2 min-w-0 flex-1 hover:text-white transition-colors"
              title={workspaceCwd}
            >
              <Folder size={14} className="shrink-0 text-[#d29922]" />
              <span className="min-w-0 flex-1 text-left">
                <span className="block truncate text-[11px] font-medium text-[#e6edf3]">
                  {workspaceCwd.split(/[\\/]/).filter(Boolean).pop()}
                </span>
                <span className="block text-[9px] text-[#6e7681]">Active workspace</span>
              </span>
            </button>
            {onClearWorkspace && (
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  onClearWorkspace();
                }}
                className="p-1 text-[#6e7681] hover:text-[#f85149] rounded transition-colors"
                title="Detach workspace from this chat"
                aria-label="Detach workspace"
              >
                <X size={12} />
              </button>
            )}
          </div>
        ) : (
          <button
            type="button"
            onClick={onSelectWorkspace}
            className="mb-2 flex w-full items-center gap-2 rounded-md border border-dashed border-[#30363d] bg-[#161b22]/40 px-2.5 py-2 text-left hover:border-[#58a6ff]/50 hover:bg-[#21262d] transition-colors"
            title="Click to select a workspace folder"
          >
            <FolderPlus size={14} className="shrink-0 text-[#8b949e]" />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[11px] text-[#8b949e]">No workspace</span>
              <span className="block text-[9px] text-[#6e7681]">Click to open folder</span>
            </span>
          </button>
        )}
        <div className="flex items-center gap-1">
          <button type="button" onClick={onOpenSettings} className="min-w-0 flex-1 rounded px-1.5 py-1 text-left hover:bg-[#161b22]">
            <span className="block truncate text-[10px] text-[#8b949e]">{effectiveConfig?.provider.name || 'No provider'}</span>
            <span className="block truncate font-mono text-[10px] text-[#58a6ff]">{effectiveConfig?.provider.model || 'Configure model'}</span>
          </button>
          <button type="button" onClick={onOpenSettings} className="icon-button" aria-label="Open settings"><Settings size={15} /></button>
        </div>
      </div>
    </aside>
  );
});
