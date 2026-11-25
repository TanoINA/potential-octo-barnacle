import { memo, useEffect, useMemo, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { fromMarkdown } from 'mdast-util-from-markdown';
import { Check, ChevronDown, ChevronRight, Download, Folder, FolderPlus, GitBranch, GitCommit, Layers, Loader2, MessageSquare, MoreHorizontal, Palette, Pencil, Pin, Plus, RefreshCw, Search, Settings, Sparkles, Trash2, X } from 'lucide-react';
import type { DeepSearchResult, EffectiveRunConfig, Session, SessionActivity } from '../types/agent';
import { useTheme } from '../themes';
import { AetherLogo } from './AetherLogo';

interface SidebarProps {
  sessions: Session[];
  sessionActivities?: Record<string, SessionActivity>;
  currentSessionId: string;
  onNewSession: () => void;
  onSelectSession: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onRenameSession: (id: string, title: string) => void;
  onAutoTitleSession?: (id: string) => Promise<string | null | void>;
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

interface PreviewNode {
  type: string;
  value?: string;
  alt?: string | null;
  children?: PreviewNode[];
}

const previewCache = new Map<string, string>();
const MAX_PREVIEW_CACHE = 256;

function plainSessionPreview(markdown: string): string {
  const cached = previewCache.get(markdown);
  if (cached !== undefined) return cached;
  const text = (node: PreviewNode): string => {
    if (node.type === 'break') return ' ';
    const content = node.value ?? node.alt ?? node.children?.map(text).join('') ?? '';
    return ['paragraph', 'heading', 'code', 'listItem', 'thematicBreak'].includes(node.type)
      ? `${content} `
      : content;
  };
  // Keep HTML node values literal; React escapes the resulting plain string.
  const rendered = text(fromMarkdown(markdown)).replace(/\s+/g, ' ').trim();
  if (previewCache.size >= MAX_PREVIEW_CACHE) {
    const oldest = previewCache.keys().next().value;
    if (oldest !== undefined) previewCache.delete(oldest);
  }
  previewCache.set(markdown, rendered);
  return rendered;
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
  onAutoTitleSession,
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
  const { preset, cycleTheme } = useTheme();
  const [gitDrawerOpen, setGitDrawerOpen] = useState(false);
  const [collapsedProjects, setCollapsedProjects] = useState<Record<string, boolean>>({});
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
  const hasWorkspaceSessions = !search.trim() && recentSessions.some((s) => Boolean(s.workspaceCwd?.trim()));
  const groupedRecentSessions = useMemo(() => {
    if (!hasWorkspaceSessions) return [];
    const groups = new Map<string, { key: string; label: string; items: Session[] }>();
    for (const session of recentSessions) {
      const cwd = (session.workspaceCwd || '').trim();
      const key = cwd || '__general__';
      let entry = groups.get(key);
      if (!entry) {
        const label = cwd ? (cwd.split(/[\\/]/).filter(Boolean).pop() || cwd) : 'General';
        entry = { key, label, items: [] };
        groups.set(key, entry);
      }
      entry.items.push(session);
    }
    return Array.from(groups.values());
  }, [hasWorkspaceSessions, recentSessions]);

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
      <div key={session.id} className={`group relative mb-0.5 rounded-lg border border-transparent transition-colors duration-100 hover:transition-none ${active ? 'bg-[var(--ui-row-active)] text-[var(--ui-text-primary)]' : 'hover:bg-[var(--ui-row-hover)]'}`}>
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
              className="min-w-0 flex-1 rounded-md border border-[var(--ui-stroke-primary)] bg-[var(--ui-bg-chrome)] px-2 py-1 text-[13px] text-[var(--ui-text-primary)] outline-none"
              aria-label="Session title"
            />
            <button type="button" className="icon-button rounded-lg" onClick={() => commitRename(session.id)} aria-label="Save title"><Check size={13} /></button>
          </div>
        ) : deleteId === session.id ? (
          <div className="flex items-center gap-1.5 px-2.5 py-2 text-[13px] text-[var(--ui-red)]">
            <span className="flex-1">Delete this chat?</span>
            <button type="button" className="rounded-md bg-[var(--ui-red)] px-2 py-1 text-xs font-medium text-white" onClick={() => { onDeleteSession(session.id); setDeleteId(null); }}>Delete</button>
            <button type="button" className="rounded-md px-2 py-1 text-xs text-[var(--ui-text-secondary)] hover:bg-[var(--ui-row-hover)]" onClick={() => setDeleteId(null)}>Cancel</button>
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
              title={`${session.title || 'Untitled'}\n${session.pinned ? 'Pinned chat (Shift+Click to unpin)' : 'Shift+Click to pin chat to top'}`}
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
                <span className="ml-5 mt-0.5 block truncate text-xs text-[#92929c]">{plainSessionPreview(session.preview)}</span>
              )}
              {deepResult?.snippet && (
                <div
                  className="ml-5 mt-1 rounded-lg border border-[#343438]/60 bg-[#141416] px-2 py-1 text-xs text-[#a1a1aa] line-clamp-2 leading-relaxed"
                >
                  {renderSnippetWithHighlights(deepResult.snippet)}
                </div>
              )}
            </button>
            {onTogglePinSession && (
              <button
                type="button"
                data-session-pin-button
                className={`icon-button rounded-lg mr-0.5 opacity-0 focus:opacity-100 group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-[var(--ui-row-hover)] ${
                  session.pinned ? 'text-[var(--ui-text-primary)] !opacity-100' : 'text-[var(--ui-text-tertiary)] hover:text-[var(--ui-text-primary)]'
                }`}
                onClick={(e) => {
                  e.stopPropagation();
                  onTogglePinSession(session.id);
                }}
                aria-label={session.pinned ? `Unpin ${session.title}` : `Pin ${session.title} to top`}
                title={session.pinned ? 'Unpin chat' : 'Pin chat to top'}
              >
                <Pin size={12} className={session.pinned ? 'fill-current text-[var(--ui-text-primary)]' : ''} />
              </button>
            )}
            <button
              type="button"
              data-session-menu-trigger
              className="icon-button rounded-lg text-[var(--ui-text-tertiary)] hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)] mr-1 opacity-0 focus:opacity-100 group-focus-within:opacity-100 group-hover:opacity-100"
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
          <div data-session-menu className="ui-menu fixed z-[60] w-44 rounded-xl border border-[var(--stroke-nous)] bg-[var(--ui-bg-elevated)] p-1 shadow-nous" style={menuPosition}>
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
            {onAutoTitleSession && (
              <button
                type="button"
                className="session-action rounded-lg text-xs focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#a1a1aa]"
                onClick={() => {
                  void onAutoTitleSession(session.id);
                  setMenuId(null);
                }}
              >
                <Sparkles size={12} /> Auto-generate title
              </button>
            )}
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
    <aside className={`${overlay ? 'ui-sidebar-enter fixed inset-y-0 left-0 z-40 shadow-nous' : 'relative'} flex h-full w-64 shrink-0 flex-col [&_button]:focus-visible:outline [&_button]:focus-visible:outline-2 [&_button]:focus-visible:outline-offset-2 [&_button]:focus-visible:outline-[var(--ui-accent)] border-r border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-sidebar)] text-[var(--ui-text-primary)]`}>
      <div className="flex h-10 shrink-0 items-center justify-between border-b border-[var(--ui-stroke-tertiary)] px-3">
        <div className="flex items-center gap-2">
          <div className="flex h-6 w-6 items-center justify-center rounded-md bg-[var(--ui-control-fill)]">
            <AetherLogo size={14} />
          </div>
          <span className="text-[13px] font-semibold tracking-tight text-[var(--ui-text-primary)]">AetherAgent</span>
        </div>
        <button type="button" onClick={onNewSession} className="icon-button rounded-md" aria-label="New chat" title="New chat (Ctrl+N)"><Plus size={15} /></button>
      </div>

      <div className="space-y-2 px-3 pb-2 pt-2.5">
        <div className="flex items-center gap-1.5">
          <button type="button" onClick={onNewSession} className="flex min-w-0 flex-1 items-center justify-center gap-1.5 rounded-lg bg-[var(--ui-control-fill)] py-1.5 text-[12.5px] font-medium text-[var(--ui-text-primary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-control-hover)]"><Plus size={13} /> New chat</button>
          {onOpenArtifactsLibrary && (
            <button
              type="button"
              onClick={onOpenArtifactsLibrary}
              className="flex h-8 w-8 items-center justify-center rounded-lg bg-[var(--ui-control-fill)] text-[var(--ui-text-secondary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-control-hover)] hover:text-[var(--ui-text-primary)]"
              title="Artifacts Library"
              aria-label="Open global artifacts library"
            >
              <Layers size={14} />
            </button>
          )}
        </div>
        <label aria-busy={isDeepSearching} className="flex items-center gap-2 border-b border-[var(--ui-stroke-tertiary)] px-0.5 py-0.5 transition-colors focus-within:border-[var(--ui-accent)]">
          <Search size={13} className={isDeepSearching ? 'text-[var(--ui-accent)]' : 'text-[var(--ui-text-quaternary)]'} />
          <input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            aria-label="Search messages and artifacts"
            placeholder="Search messages & artifacts"
            className="h-7 min-w-0 flex-1 bg-transparent text-[12.5px] text-[var(--ui-text-primary)] outline-none placeholder:text-[var(--ui-text-quaternary)]"
          />
          {search && <button type="button" onClick={() => setSearch('')} aria-label="Clear search" className="rounded p-0.5 text-[var(--ui-text-tertiary)] hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]"><X size={12} /></button>}
        </label>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-2">
        {filtered.length === 0 ? (
          <div className="px-3 py-8 text-center text-[13px] text-[var(--ui-text-tertiary)]">{search ? 'No matching conversations' : 'No saved conversations yet'}</div>
        ) : (
          <>
            {pinnedSessions.length > 0 && (
              <div className="mb-3">
                <div className="flex items-center justify-between px-2 py-1 text-[11px] font-medium text-[var(--ui-text-tertiary)]">
                  <span className="flex items-center gap-1.5">
                    <Pin size={11} className="text-[var(--ui-text-tertiary)]" />
                    <span>Pinned ({pinnedSessions.length})</span>
                  </span>
                  <span className="text-[10.5px] font-normal normal-case text-[var(--ui-text-quaternary)]">Shift-click to toggle</span>
                </div>
                {pinnedSessions.map(renderSessionItem)}
              </div>
            )}

            <div>
              <div className="flex items-center justify-between px-2 py-1 text-[11px] font-medium text-[var(--ui-text-tertiary)]">
                <span>Recent conversations ({recentSessions.length})</span>
              </div>
              {recentSessions.length === 0 && pinnedSessions.length > 0 && (
                <div className="px-3 py-3 text-center text-[13px] text-[var(--ui-text-tertiary)]">All matching chats are pinned above</div>
              )}
              {hasWorkspaceSessions ? (
                <div className="space-y-1.5">
                  {groupedRecentSessions.map((group) => {
                    const isCollapsed = Boolean(collapsedProjects[group.key]);
                    return (
                      <div key={group.key}>
                        <button
                          type="button"
                          aria-expanded={!isCollapsed}
                          onClick={() => setCollapsedProjects((prev) => ({ ...prev, [group.key]: !prev[group.key] }))}
                          className="flex w-full items-center justify-between rounded-md px-2 py-1 text-left text-[11px] font-medium text-[var(--ui-text-tertiary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]"
                        >
                          <span className="flex min-w-0 items-center gap-1.5">
                            {isCollapsed ? <ChevronRight size={12} className="shrink-0" /> : <ChevronDown size={12} className="shrink-0" />}
                            <Folder size={11} className="shrink-0 opacity-75" />
                            <span className="truncate">{group.label}</span>
                          </span>
                          <span className="ml-2 shrink-0 font-mono text-[10px] text-[var(--ui-text-quaternary)]">{group.items.length}</span>
                        </button>
                        {!isCollapsed && (
                          <div className="mt-0.5 pl-1">
                            {group.items.map(renderSessionItem)}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              ) : (
                recentSessions.map(renderSessionItem)
              )}
            </div>
          </>
        )}
      </div>

      <div className="border-t border-[var(--ui-stroke-tertiary)] p-2.5">
        {workspaceCwd ? (
          <div className="mb-2 flex w-full flex-col gap-1 rounded-lg bg-[var(--ui-control-fill)] p-2 text-left group">
            <div className="flex items-center gap-1.5 w-full">
              <button
                type="button"
                onClick={onSelectWorkspace}
                className="flex items-center gap-2 min-w-0 flex-1 rounded-lg p-1 hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)] transition-colors"
                title={workspaceCwd}
              >
                <Folder size={14} className="shrink-0 text-[var(--ui-text-tertiary)]" />
                <span className="min-w-0 flex-1 text-left">
                  <span className="block truncate text-[13px] font-medium text-[var(--ui-text-primary)]">
                    {workspaceCwd.split(/[\\/]/).filter(Boolean).pop()}
                  </span>
                  <span className="block text-xs text-[var(--ui-text-quaternary)]">Active workspace</span>
                </span>
              </button>
              {onClearWorkspace && (
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onClearWorkspace();
                  }}
                  className="p-1 text-[var(--ui-text-quaternary)] hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-red)] rounded-lg transition-colors"
                  title="Detach workspace from this chat"
                  aria-label="Detach workspace"
                >
                  <X size={12} />
                </button>
              )}
            </div>

            {gitStatus?.isRepo && (
              <div className="mt-1 border-t border-[var(--ui-stroke-tertiary)] pt-1.5" data-testid="sidebar-git-section">
                <div className="flex items-center justify-between text-[13px]">
                  <span className="flex items-center gap-1 font-mono text-[var(--ui-text-tertiary)]">
                    <GitBranch size={11} className="text-[var(--ui-text-tertiary)]" />
                    <span className="truncate max-w-[80px] text-[var(--ui-text-secondary)]" title={`Branch: ${gitStatus.branch}`}>{gitStatus.branch}</span>
                  </span>
                  <button
                    type="button"
                    onClick={() => setGitDrawerOpen((v) => !v)}
                    className={`inline-flex items-center gap-1 rounded-lg px-1.5 py-0.5 font-medium transition-colors ${
                      gitStatus.hasChanges
                        ? 'bg-[var(--ui-yellow)]/15 text-[var(--ui-text-secondary)] hover:bg-[var(--ui-yellow)]/25'
                        : 'bg-[var(--ui-green)]/15 text-[var(--ui-green)] hover:bg-[var(--ui-green)]/25'
                    }`}
                    title={gitStatus.summary}
                    aria-label="Toggle git details"
                  >
                    <span className={`h-1.5 w-1.5 rounded-full ${gitStatus.hasChanges ? 'bg-[var(--ui-yellow)]' : 'bg-[var(--ui-green)]'}`} />
                    <span>{gitStatus.hasChanges ? `${gitStatus.files.length} changes` : 'Clean'}</span>
                  </button>
                </div>

                {gitDrawerOpen && (
                  <div className="mt-2 rounded-lg border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-elevated)] p-2 text-[13px]" data-testid="git-drawer">
                    <div className="mb-1.5 flex items-center justify-between text-[13px] text-[var(--ui-text-tertiary)]">
                      <span className="font-medium">Changes ({gitStatus.files.length})</span>
                      {onRefreshGitStatus && (
                        <button
                          type="button"
                          onClick={() => void onRefreshGitStatus()}
                          className="rounded-lg p-1 hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)] transition-colors"
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
                              file.status === 'M' ? 'bg-[var(--ui-yellow)]/20 text-[var(--ui-yellow)]' :
                              file.status === 'D' ? 'bg-[var(--ui-red)]/20 text-[var(--ui-red)]' :
                              'bg-[var(--ui-green)]/20 text-[var(--ui-green)]'
                            }`}>
                              {file.status}
                            </span>
                            <span className="truncate text-[var(--ui-text-secondary)]" title={file.path}>
                              {file.path}
                            </span>
                          </div>
                        ))}
                      </div>
                    ) : (
                      <div className="py-1 text-[13px] text-[var(--ui-text-tertiary)]">Working tree clean</div>
                    )}

                    {onCommitWorkspace && gitStatus.hasChanges && (
                      <div className="mt-1 space-y-1.5">
                        <input
                          type="text"
                          value={commitMsg}
                          onChange={(e) => setCommitMsg(e.target.value)}
                          placeholder="Commit message…"
                          className="w-full rounded-lg border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-editor)] px-2 py-1 text-[13px] text-[var(--ui-text-primary)] placeholder:text-[var(--ui-text-quaternary)] outline-none focus:border-[var(--ui-accent)]"
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
                          className="w-full flex items-center justify-center gap-1.5 rounded-lg bg-[var(--ui-accent)] py-1 text-[13px] font-medium text-[var(--ui-accent-fg)] hover:opacity-90 disabled:opacity-40 disabled:cursor-not-allowed transition-opacity"
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
            className="mb-2 flex w-full items-center gap-2 rounded-lg border border-dashed border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] px-2.5 py-2 text-left hover:border-[var(--ui-stroke-secondary)] hover:bg-[var(--ui-row-hover)] transition-colors"
            title="Click to select a workspace folder"
          >
            <FolderPlus size={14} className="shrink-0 text-[var(--ui-text-tertiary)]" />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[13px] text-[var(--ui-text-tertiary)]">No workspace</span>
              <span className="block text-xs text-[var(--ui-text-quaternary)]">Click to open folder</span>
            </span>
          </button>
        )}
        <div className="flex items-center gap-1">
          <button type="button" onClick={onOpenSettings} className="min-w-0 flex-1 rounded-lg px-1.5 py-1 text-left hover:bg-[var(--ui-row-hover)]">
            <span className="block truncate text-[13px] text-[var(--ui-text-tertiary)]">{effectiveConfig?.provider.name || 'No provider'}</span>
            <span className="block truncate text-xs text-[var(--ui-text-secondary)]">Settings</span>
          </button>
          <button
            type="button"
            onClick={() => cycleTheme()}
            className="icon-button rounded-lg"
            aria-label="Switch color theme"
            title={`Theme: ${preset.label} (click to switch)`}
          >
            <Palette size={14} />
          </button>
          <button type="button" onClick={onOpenSettings} className="icon-button rounded-lg" aria-label="Open settings"><Settings size={15} /></button>
        </div>
      </div>
    </aside>
  );
});
