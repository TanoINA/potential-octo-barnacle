import { memo, useEffect, useMemo, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { Check, Download, Folder, FolderPlus, GitBranch, GitCommit, Layers, Loader2, MessageSquare, MoreHorizontal, Pencil, Pin, Plus, RefreshCw, Search, Settings, Trash2, X } from 'lucide-react';
import type { DeepSearchResult, EffectiveRunConfig, Session, SessionActivity } from '../types/agent';
import { AetherLogo } from './AetherLogo';

interface SidebarProps {
  sessions: Session[];
  sessionActivities?: Record<string, SessionActivity>;
  currentSessionId: string;
  onNewSession: () => void;
  onSelectSession: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onRenameSession: (id: string, title: string) => void;
  onTogglePinSession?: (id: string) => void;
  onOpenSearchResult?: (result: DeepSearchResult) => Promise<boolean>;
  onDeepSearch?: (query: string) => Promise<DeepSearchResult[]>;
  onExportSession: (id: string, format: 'json' | 'markdown' | 'html') => void;
  onOpenSettings: () => void;
  onOpenArtifactsLibrary?: () => void;
  workspaceCwd: string;
  onSelectWorkspace: () => void;
  onClearWorkspace?: () => void;
  effectiveConfig: EffectiveRunConfig | null;
  overlay?: boolean;
  gitStatus?: import('../types/agent').GitRepoStatus | null;
  onRefreshGitStatus?: () => Promise<import('../types/agent').GitRepoStatus | null>;
  onCommitWorkspace?: (message: string) => Promise<string>;
}

const SNIPPET_HIGHLIGHT_CLASS = 'bg-[#3f3f46] text-[#f4f4f5] font-medium px-0.5 rounded-lg';

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
  sessionActivities,
  onNewSession,
  onSelectSession,
  onDeleteSession,
  onRenameSession,
  onTogglePinSession,
  onDeepSearch,
  onOpenSearchResult,
  onExportSession,
  onOpenSettings,
  onOpenArtifactsLibrary,
  workspaceCwd,
  onSelectWorkspace,
  onClearWorkspace,
  effectiveConfig,
  overlay = false,
  gitStatus,
  onRefreshGitStatus,
  onCommitWorkspace,
}: SidebarProps) {
  const [gitDrawerOpen, setGitDrawerOpen] = useState(false);
  const [commitMsg, setCommitMsg] = useState('');
  const [isCommitting, setIsCommitting] = useState(false);

  const handleCommit = async () => {
    if (!(commitMsg || '').trim() || !onCommitWorkspace || isCommitting) return;
    setIsCommitting(true);
    try {
      await onCommitWorkspace((commitMsg || '').trim());
      setCommitMsg('');
      setGitDrawerOpen(false);
    } catch {
      // error handled in useAgent
    } finally {
      setIsCommitting(false);
    }
  };
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
    const q = (search || '').trim();
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
    const query = (search || '').trim().toLowerCase();
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
    if ((renameValue || '').trim()) onRenameSession(id, (renameValue || '').trim());
    setRenamingId(null);
  };

  const renderSessionItem = (session: Session) => {
    const active = session.id === currentSessionId;
    const renaming = renamingId === session.id;
    const deepResult = deepResults[session.id];
    const activity = sessionActivities?.[session.id];
    const statusLabel = activity && { running: 'Running', saving: 'Saving', completed: 'Completed', cancelled: 'Stopped', failed: 'Failed' }[activity];

    return (
      <div key={session.id} className={`group relative mb-1 rounded-lg border ${active ? 'border-[#45454b] bg-[#27272a] text-[#f4f4f5]' : 'border-transparent hover:bg-[#1c1c1f]'}`}>
        {renaming ? (
          <div className="flex items-center gap-1 p-1.5">
            <input
              autoFocus
              value={renameValue}
              onChange={(event) => setRenameValue(event.target.value)}
              onKeyDown={(event) => {
                if (event.nativeEvent.isComposing) return;
                if (event.key === 'Enter') commitRename(session.id);
                if (event.key === 'Escape') setRenamingId(null);
              }}
              className="min-w-0 flex-1 rounded-lg border border-[#d4d4d8] bg-[#141416] px-2 py-1 text-[13px] outline-none"
              aria-label="Session title"
            />
            <button type="button" className="icon-button rounded-lg text-[#a1a1aa] hover:bg-[#27272a] hover:text-[#f4f4f5]" onClick={() => commitRename(session.id)} aria-label="Save title"><Check size={13} /></button>
          </div>
        ) : deleteId === session.id ? (
          <div className="flex items-center gap-1.5 px-2.5 py-2 text-[13px] text-[#f85149]">
            <span className="flex-1">Delete this chat?</span>
            <button type="button" className="rounded-lg bg-[#da3633] px-2 py-1 text-white" onClick={() => { onDeleteSession(session.id); setDeleteId(null); }}>Delete</button>
            <button type="button" className="rounded-lg px-2 py-1 text-[#a1a1aa] hover:bg-[#343438]" onClick={() => setDeleteId(null)}>Cancel</button>
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
                if (deepResult && onOpenSearchResult) void onOpenSearchResult(deepResult);
                else onSelectSession(session.id);
              }}
              aria-current={active ? 'page' : undefined}
              className="min-w-0 flex-1 rounded-lg px-2.5 py-2 text-left"
              title={session.pinned ? 'Pinned chat (Shift+Click to unpin)' : 'Shift+Click to pin chat to top'}
            >
              <span className="flex items-center gap-2">
                {session.pinned ? (
                  <Pin size={12} className="shrink-0 text-[#a1a1aa]" />
                ) : (
                  <MessageSquare size={13} className={`shrink-0 ${active ? 'text-[#d4d4d8]' : 'text-[#92929c]'}`} />
                )}
                <span className="min-w-0 flex-1 truncate text-[13px]">{session.title || 'Untitled'}</span>
                {statusLabel && <span aria-label={`Chat status: ${statusLabel}`} title={statusLabel}
                  className={`inline-flex shrink-0 items-center gap-1 text-[10px] ${activity === 'failed' ? 'text-[#f85149]' : activity === 'completed' ? 'text-[#3fb950]' : 'text-[#a1a1aa]'}`}>
                  {activity === 'running' || activity === 'saving' ? <Loader2 size={10} className="animate-spin" /> : activity === 'completed' ? <Check size={10} /> : null}
                  {statusLabel}
                </span>}
              </span>
              {session.preview && !deepResult && (
                <span className="ml-5 mt-0.5 block truncate text-xs text-[#92929c]">{session.preview}</span>
              )}
              {deepResult?.snippet && (
                <div
                  className="ml-5 mt-1 rounded-lg border border-[#343438]/60 bg-[#141416] px-2 py-1 text-xs text-[#a1a1aa] line-clamp-2 leading-relaxed"
                >
                  {renderSnippetWithHighlights(deepResult.snippet)}
                </div>
              )}
            </button>
            <button
              type="button"
              data-session-menu-trigger
              className="icon-button rounded-lg text-[#a1a1aa] hover:bg-[#27272a] hover:text-[#f4f4f5] mr-1 opacity-0 focus:opacity-100 group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-[#343438]"
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
          <div data-session-menu className="fixed z-[60] w-44 rounded-lg border border-[#343438] bg-[#1c1c1f] p-1 shadow-lg" style={menuPosition}>
            {onTogglePinSession && (
              <button
                type="button"
                className="session-action rounded-lg text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#a1a1aa]"
                onClick={() => {
                  onTogglePinSession(session.id);
                  setMenuId(null);
                }}
              >
                <Pin size={12} className={session.pinned ? 'text-[#a1a1aa]' : ''} />
                {session.pinned ? 'Unpin chat' : 'Pin chat to top'}
              </button>
            )}
            <button type="button" className="session-action rounded-lg text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#a1a1aa]" onClick={() => { setRenamingId(session.id); setRenameValue(session.title); setMenuId(null); }}><Pencil size={12} /> Rename</button>
            <button type="button" className="session-action rounded-lg text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#a1a1aa]" onClick={() => { onExportSession(session.id, 'html'); setMenuId(null); }}><Download size={12} /> Export HTML (.html)</button>
            <button type="button" className="session-action rounded-lg text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#a1a1aa]" onClick={() => { onExportSession(session.id, 'markdown'); setMenuId(null); }}><Download size={12} /> Export Markdown</button>
            <button type="button" className="session-action rounded-lg text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#a1a1aa]" onClick={() => { onExportSession(session.id, 'json'); setMenuId(null); }}><Download size={12} /> Export JSON</button>
            <button type="button" className="session-action rounded-lg text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#a1a1aa] text-[#f85149]" onClick={() => { setDeleteId(session.id); setMenuId(null); }}><Trash2 size={12} /> Delete</button>
          </div>,
          document.body,
        )}
      </div>
    );
  };

  return (
    <aside className={`${overlay ? 'ui-sidebar-enter fixed inset-y-0 left-0 z-40 shadow-xl' : 'relative'} flex h-full w-64 shrink-0 flex-col [&_button]:focus-visible:outline [&_button]:focus-visible:outline-2 [&_button]:focus-visible:outline-offset-2 [&_button]:focus-visible:outline-[#a1a1aa] border-r border-[#343438] bg-[#141416] text-[#e4e4e7]`}>
      <div className="flex h-14 shrink-0 items-center justify-between border-b border-[#343438] px-3">
        <div className="flex items-center gap-2.5">
          <div className="flex h-7 w-7 items-center justify-center rounded-lg border border-[#343438] bg-[#1c1c1f]">
            <AetherLogo size={15} />
          </div>
          <span className="text-[13px] font-semibold text-[#e4e4e7]">AetherAgent</span>
        </div>
        <button type="button" onClick={onNewSession} className="icon-button rounded-lg text-[#a1a1aa] hover:bg-[#27272a] hover:text-[#f4f4f5]" aria-label="New chat" title="New chat (Ctrl+N)"><Plus size={16} /></button>
      </div>

      <div className="space-y-2 p-3">
        <div className="flex items-center gap-1.5">
          <button type="button" onClick={onNewSession} className="flex min-w-0 flex-1 items-center justify-center gap-2 rounded-lg border border-[#343438] bg-[#1c1c1f] py-2 text-[13px] font-medium hover:bg-[#27272a] transition-colors"><Plus size={14} /> New chat</button>
          {onOpenArtifactsLibrary && (
            <button
              type="button"
              onClick={onOpenArtifactsLibrary}
              className="flex items-center justify-center rounded-lg border border-[#343438] bg-[#1c1c1f] p-2 text-[13px] text-[#a1a1aa] hover:bg-[#27272a] hover:text-[#f4f4f5] transition-colors"
              title="Artifacts Library"
              aria-label="Open global artifacts library"
            >
              <Layers size={14} />
            </button>
          )}
        </div>
        <label aria-busy={isDeepSearching} className="flex items-center gap-2 rounded-lg border border-[#343438] bg-[#1c1c1f] px-2.5 focus-within:border-[#d4d4d8]">
          <Search size={13} className={isDeepSearching ? 'text-[#d4d4d8]' : 'text-[#92929c]'} />
          <input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            aria-label="Search messages and artifacts"
            placeholder="Search messages & artifacts"
            className="h-8 min-w-0 flex-1 bg-transparent text-[13px] outline-none placeholder:text-[#92929c]"
          />
          {search && <button type="button" onClick={() => setSearch('')} aria-label="Clear search" className="rounded-lg p-1 hover:bg-[#343438]"><X size={12} className="text-[#a1a1aa]" /></button>}
        </label>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {filtered.length === 0 ? (
          <div className="px-3 py-8 text-center text-[13px] text-[#92929c]">{search ? 'No matching conversations' : 'No saved conversations yet'}</div>
        ) : (
          <>
            {pinnedSessions.length > 0 && (
              <div className="mb-3">
                <div className="flex items-center justify-between px-2 py-1 text-xs font-medium text-[#a1a1aa]">
                  <span className="flex items-center gap-1.5">
                    <Pin size={11} className="text-[#a1a1aa]" />
                    <span>Pinned ({pinnedSessions.length})</span>
                  </span>
                  <span className="text-xs font-normal normal-case text-[#a1a1aa]">Shift-click to toggle</span>
                </div>
                {pinnedSessions.map(renderSessionItem)}
              </div>
            )}

            <div>
              <div className="flex items-center justify-between px-2 py-1 text-xs font-medium text-[#92929c]">
                <span>Recent conversations ({recentSessions.length})</span>
              </div>
              {recentSessions.length === 0 && pinnedSessions.length > 0 && (
                <div className="px-3 py-3 text-center text-[13px] text-[#92929c]">All matching chats are pinned above</div>
              )}
              {recentSessions.map(renderSessionItem)}
            </div>
          </>
        )}
      </div>

      <div className="border-t border-[#343438] p-3">
        {workspaceCwd ? (
          <div className="mb-2 flex w-full flex-col gap-1 rounded-lg border border-[#343438] bg-[#1c1c1f] p-2 text-left group">
            <div className="flex items-center gap-1.5 w-full">
              <button
                type="button"
                onClick={onSelectWorkspace}
                className="flex items-center gap-2 min-w-0 flex-1 rounded-lg p-1 hover:bg-[#27272a] hover:text-white transition-colors"
                title={workspaceCwd}
              >
                <Folder size={14} className="shrink-0 text-[#a1a1aa]" />
                <span className="min-w-0 flex-1 text-left">
                  <span className="block truncate text-[13px] font-medium text-[#e4e4e7]">
                    {workspaceCwd.split(/[\\/]/).filter(Boolean).pop()}
                  </span>
                  <span className="block text-xs text-[#92929c]">Active workspace</span>
                </span>
              </button>
              {onClearWorkspace && (
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onClearWorkspace();
                  }}
                  className="p-1 text-[#92929c] hover:bg-[#27272a] hover:text-[#f85149] rounded-lg transition-colors"
                  title="Detach workspace from this chat"
                  aria-label="Detach workspace"
                >
                  <X size={12} />
                </button>
              )}
            </div>

            {gitStatus?.isRepo && (
              <div className="mt-1 border-t border-[#343438]/50 pt-1.5" data-testid="sidebar-git-section">
                <div className="flex items-center justify-between text-[13px]">
                  <span className="flex items-center gap-1 font-mono text-[#a1a1aa]">
                    <GitBranch size={11} className="text-[#a1a1aa]" />
                    <span className="truncate max-w-[80px] text-[#d4d4d8]" title={`Branch: ${gitStatus.branch}`}>{gitStatus.branch}</span>
                  </span>
                  <button
                    type="button"
                    onClick={() => setGitDrawerOpen((v) => !v)}
                    className={`inline-flex items-center gap-1 rounded-lg px-1.5 py-0.5 font-medium transition-colors ${
                      gitStatus.hasChanges
                        ? 'bg-[#d29922]/15 text-[#a1a1aa] hover:bg-[#d29922]/25'
                        : 'bg-[#3fb950]/15 text-[#3fb950] hover:bg-[#3fb950]/25'
                    }`}
                    title={gitStatus.summary}
                    aria-label="Toggle git details"
                  >
                    <span className={`h-1.5 w-1.5 rounded-full ${gitStatus.hasChanges ? 'bg-[#d29922]' : 'bg-[#3fb950]'}`} />
                    <span>{gitStatus.hasChanges ? `${gitStatus.files.length} changes` : 'Clean'}</span>
                  </button>
                </div>

                {gitDrawerOpen && (
                  <div className="mt-2 rounded-lg border border-[#343438] bg-[#141416] p-2 text-[13px]" data-testid="git-drawer">
                    <div className="mb-1.5 flex items-center justify-between text-[13px] text-[#a1a1aa]">
                      <span className="font-medium">Changes ({gitStatus.files.length})</span>
                      {onRefreshGitStatus && (
                        <button
                          type="button"
                          onClick={() => void onRefreshGitStatus()}
                          className="rounded-lg p-1 hover:bg-[#27272a] hover:text-[#f4f4f5] transition-colors"
                          title="Refresh git status"
                          aria-label="Refresh git status"
                        >
                          <RefreshCw size={10} />
                        </button>
                      )}
                    </div>
                    {gitStatus.files.length > 0 ? (
                      <div className="max-h-28 overflow-y-auto space-y-1 mb-2 scrollbar-thin">
                        {gitStatus.files.map((file) => (
                          <div key={file.path} className="flex items-center gap-1.5 text-[13px]">
                            <span className={`font-mono text-[13px] font-bold px-1 rounded-lg ${
                              file.status === 'M' ? 'bg-[#d29922]/20 text-[#a1a1aa]' :
                              file.status === 'D' ? 'bg-[#f85149]/20 text-[#f85149]' :
                              'bg-[#3fb950]/20 text-[#3fb950]'
                            }`}>
                              {file.status}
                            </span>
                            <span className="truncate text-[#d4d4d8]" title={file.path}>
                              {file.path}
                            </span>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="py-1 text-[13px] text-[#a1a1aa]">Working tree clean</div>
                    )}

                    {onCommitWorkspace && gitStatus.hasChanges && (
                      <div className="mt-1 space-y-1.5">
                        <input
                          type="text"
                          value={commitMsg}
                          onChange={(e) => setCommitMsg(e.target.value)}
                          placeholder="Commit message…"
                          className="w-full rounded-lg border border-[#343438] bg-[#1c1c1f] px-2 py-1 text-[13px] text-[#e4e4e7] placeholder:text-[#92929c] outline-none focus:border-[#d4d4d8]"
                          onKeyDown={(e) => {
                            if (e.nativeEvent.isComposing) return;
                            if (e.key === 'Enter' && (commitMsg || '').trim() && !isCommitting) {
                              void handleCommit();
                            }
                          }}
                        />
                        <button
                          type="button"
                          disabled={!(commitMsg || '').trim() || isCommitting}
                          onClick={() => void handleCommit()}
                          className="w-full flex items-center justify-center gap-1.5 rounded-lg border border-[#45454b] bg-[#343438] py-1 text-[13px] font-medium text-white hover:bg-[#45454b] disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
                        >
                          {isCommitting ? <Loader2 size={10} className="text-current" /> : <GitCommit size={10} />}
                          <span>{isCommitting ? 'Committing…' : 'Commit all changes'}</span>
                        </button>
                      </div>
                    )}
                  </div>
                )}
              </div>
            )}
          </div>
        ) : (
          <button
            type="button"
            onClick={onSelectWorkspace}
            className="mb-2 flex w-full items-center gap-2 rounded-lg border border-dashed border-[#343438] bg-[#1c1c1f]/40 px-2.5 py-2 text-left hover:border-[#d4d4d8]/50 hover:bg-[#27272a] transition-colors"
            title="Click to select a workspace folder"
          >
            <FolderPlus size={14} className="shrink-0 text-[#a1a1aa]" />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[13px] text-[#a1a1aa]">No workspace</span>
              <span className="block text-xs text-[#92929c]">Click to open folder</span>
            </span>
          </button>
        )}
        <div className="flex items-center gap-1">
          <button type="button" onClick={onOpenSettings} className="min-w-0 flex-1 rounded-lg px-1.5 py-1 text-left hover:bg-[#1c1c1f]">
            <span className="block truncate text-[13px] text-[#a1a1aa]">{effectiveConfig?.provider.name || 'No provider'}</span>
            <span className="block truncate font-mono text-xs text-[#d4d4d8]">{effectiveConfig?.provider.model || 'Configure model'}</span>
          </button>
          <button type="button" onClick={onOpenSettings} className="icon-button rounded-lg text-[#a1a1aa] hover:bg-[#27272a] hover:text-[#f4f4f5]" aria-label="Open settings"><Settings size={15} /></button>
        </div>
      </div>
    </aside>
  );
});
