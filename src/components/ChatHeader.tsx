import React, { useEffect, useState } from 'react';
import {
  Download,
  MoreHorizontal,
  PanelLeft,
  PanelRight,
  Search,
  Settings,
  Terminal,
  Trash2,
  Users,
} from 'lucide-react';

export interface ChatHeaderProps {
  onToggleSidebar?: () => void;
  sidebarOpen?: boolean;
  sessionTitle?: string;
  totalMessageCount: number;
  onOpenLiveShell?: () => void;
  councilMode: boolean;
  setCouncilMode: React.Dispatch<React.SetStateAction<boolean>>;
  councilSecondCandidate?: unknown;
  onOpenModelPicker: () => void;
  effectiveConfig?: { provider: { model: string }; thinking?: boolean } | null;
  subagents?: Array<{ status: string }>;
  onToggleArtifacts: () => void;
  artifactCount: number;
  isArtifactOpen: boolean;
  onOpenSpotlight?: () => void;
  onOpenInChatSearch: () => void;
  onExportSession?: (format: 'html' | 'markdown' | 'json') => void;
  onOpenSettings: () => void;
  onClearConversation: () => void;
}

export const ChatHeader: React.FC<ChatHeaderProps> = ({
  onToggleSidebar,
  sidebarOpen,
  sessionTitle,
  totalMessageCount,
  onOpenLiveShell,
  councilMode,
  setCouncilMode,
  councilSecondCandidate,
  subagents,
  onToggleArtifacts,
  artifactCount,
  isArtifactOpen,
  onOpenSpotlight,
  onOpenInChatSearch,
  onExportSession,
  onOpenSettings,
  onClearConversation,
}) => {
  const [moreMenuOpen, setMoreMenuOpen] = useState(false);

  useEffect(() => {
    if (!moreMenuOpen) return;
    const handleOutside = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest('[data-more-menu]')) {
        setMoreMenuOpen(false);
      }
    };
    window.addEventListener('mousedown', handleOutside);
    return () => window.removeEventListener('mousedown', handleOutside);
  }, [moreMenuOpen]);

  return (
    <header className="z-20 flex h-10 shrink-0 items-center justify-between border-b border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-chrome)] px-2 sm:px-3">
      <div className="flex items-center gap-1.5 min-w-0 mr-1.5">
        {onToggleSidebar && (
          <button
            type="button"
            onClick={onToggleSidebar}
            className="flex h-7 w-7 items-center justify-center rounded-md border border-transparent bg-transparent text-[var(--ui-text-secondary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)] shrink-0"
            title={sidebarOpen ? 'Hide sidebar (Ctrl+B)' : 'Show sidebar (Ctrl+B)'}
          >
            <PanelLeft size={14} />
          </button>
        )}
        <div className="flex items-center gap-1.5 min-w-0 max-w-[120px] sm:max-w-[260px]">
          <h1 className="truncate text-xs font-semibold text-[var(--ui-text-primary)]">{sessionTitle || 'New Chat'}</h1>
          {totalMessageCount > 0 && (
            <span
              className="hidden sm:inline shrink-0 font-mono text-[11px] text-[var(--ui-text-quaternary)] whitespace-nowrap"
              title={`${totalMessageCount} messages`}
            >
              {totalMessageCount} messages
            </span>
          )}
        </div>
      </div>

      {/* Action toolbar */}
      <div className="flex items-center gap-1 shrink-0">
        {onOpenLiveShell && (
          <button
            type="button"
            onClick={onOpenLiveShell}
            className="flex h-7 w-7 items-center justify-center rounded-md border border-transparent bg-transparent text-[var(--ui-text-secondary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)] shrink-0"
            title="Open Live Shell terminal"
          >
            <Terminal size={13} />
          </button>
        )}

        {/* Council Mode Toggle */}
        <button
          type="button"
          onClick={() => setCouncilMode((v) => !v)}
          disabled={!councilSecondCandidate}
          className={`flex h-7 items-center gap-1 rounded-md border border-transparent px-2 text-xs transition-colors duration-100 hover:transition-none shrink-0 disabled:cursor-not-allowed disabled:opacity-40 ${
            councilMode
              ? 'bg-[var(--ui-row-active)] text-[var(--ui-text-primary)] font-medium'
              : 'bg-transparent text-[var(--ui-text-secondary)] hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]'
          }`}
          title={councilSecondCandidate ? 'Council Mode (Dual Model A/B Test)' : 'Council Mode needs at least two configured models'}
        >
          <Users size={13} />
          <span className="hidden md:inline text-xs">Council</span>
        </button>

        {/* Task / Artifacts Bubble Button */}
        {(() => {
          const runningTasks = subagents?.filter((s) => s.status === 'running').length || 0;
          return (
            <button
              type="button"
              onClick={onToggleArtifacts}
              disabled={artifactCount === 0 && runningTasks === 0}
              className={`flex h-7 items-center gap-1.5 rounded-md border border-transparent px-2 text-xs transition-colors duration-100 hover:transition-none shrink-0 ${
                isArtifactOpen
                  ? 'bg-[var(--ui-row-active)] text-[var(--ui-text-primary)] font-medium'
                  : runningTasks > 0
                  ? 'bg-[var(--ui-control-fill)] text-[var(--ui-yellow)] font-medium hover:bg-[var(--ui-row-hover)]'
                  : 'bg-transparent text-[var(--ui-text-secondary)] hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]'
              } disabled:cursor-not-allowed disabled:opacity-40 hover:disabled:text-[var(--ui-text-secondary)]`}
              title={
                isArtifactOpen
                  ? 'Close task inspector / artifacts panel'
                  : runningTasks > 0
                  ? `${runningTasks} task(s) active - click to inspect`
                  : 'Show task inspector / artifacts panel'
              }
            >
              {runningTasks > 0 ? (
                <>
                  <span className="relative flex h-2 w-2">
                    <span className="relative inline-flex rounded-full h-2 w-2 bg-[var(--ui-orange)]" />
                  </span>
                  <span className="text-[11px] font-mono font-semibold text-[var(--ui-yellow)]">{runningTasks} Active</span>
                </>
              ) : (
                <>
                  <PanelRight size={13} />
                  {artifactCount > 0 && (
                    <span className="rounded bg-[var(--ui-control-fill)] px-1 font-mono text-[10px] font-medium text-[var(--ui-accent)]">
                      {artifactCount}
                    </span>
                  )}
                </>
              )}
            </button>
          );
        })()}

        {/* More Actions Dropdown Menu (...) */}
        <div className="relative shrink-0" data-more-menu>
          <button
            type="button"
            onClick={() => setMoreMenuOpen((v) => !v)}
            className={`flex h-7 w-7 items-center justify-center rounded-md border border-transparent text-xs transition-colors duration-100 hover:transition-none ${
              moreMenuOpen
                ? 'bg-[var(--ui-row-active)] text-[var(--ui-text-primary)]'
                : 'bg-transparent text-[var(--ui-text-secondary)] hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]'
            }`}
            title="More actions (Search, Export, Settings)"
          >
            <MoreHorizontal size={14} />
          </button>
          {moreMenuOpen && (
            <div className="ui-menu absolute right-0 top-full mt-1.5 z-50 w-52 rounded-xl border border-[var(--stroke-nous)] bg-[var(--ui-bg-elevated)] p-1 shadow-nous">
              {onOpenSpotlight && (
                <button
                  type="button"
                  onClick={() => { setMoreMenuOpen(false); onOpenSpotlight(); }}
                  className="flex w-full items-center justify-between rounded-lg px-2.5 py-1.5 text-left text-xs text-[var(--ui-text-primary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)]"
                >
                  <span className="flex items-center gap-2">
                    <Search size={13} className="text-[var(--ui-text-tertiary)]" />
                    Search
                  </span>
                  <kbd className="rounded bg-[var(--ui-control-fill)] px-1 font-mono text-[9px] text-[var(--ui-text-quaternary)]">Alt+Space</kbd>
                </button>
              )}
              <button
                type="button"
                onClick={() => {
                  setMoreMenuOpen(false);
                  onOpenInChatSearch();
                }}
                className="flex w-full items-center justify-between rounded-lg px-2.5 py-1.5 text-left text-xs text-[var(--ui-text-primary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)]"
              >
                <span className="flex items-center gap-2">
                  <Search size={13} className="text-[var(--ui-text-tertiary)]" />
                  Find in chat
                </span>
                <kbd className="rounded bg-[var(--ui-control-fill)] px-1 font-mono text-[9px] text-[var(--ui-text-quaternary)]">Ctrl+F</kbd>
              </button>
              {onExportSession && (
                <>
                  <div className="my-1 border-t border-[var(--ui-stroke-tertiary)] px-2 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-[var(--ui-text-quaternary)]">Export</div>
                  <button
                    type="button"
                    onClick={() => { setMoreMenuOpen(false); onExportSession('html'); }}
                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[var(--ui-text-primary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)]"
                  >
                    <Download size={13} className="text-[var(--ui-text-tertiary)]" />
                    <span>HTML (PDF-ready)</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => { setMoreMenuOpen(false); onExportSession('markdown'); }}
                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[var(--ui-text-primary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)]"
                  >
                    <Download size={13} className="text-[var(--ui-text-tertiary)]" />
                    <span>Markdown (.md)</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => { setMoreMenuOpen(false); onExportSession('json'); }}
                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[var(--ui-text-primary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)]"
                  >
                    <Download size={13} className="text-[var(--ui-text-tertiary)]" />
                    <span>JSON transcript</span>
                  </button>
                </>
              )}
              <div className="my-1 border-t border-[var(--ui-stroke-tertiary)]" />
              <button
                type="button"
                onClick={() => { setMoreMenuOpen(false); onOpenSettings(); }}
                className="flex w-full items-center justify-between rounded-lg px-2.5 py-1.5 text-left text-xs text-[var(--ui-text-primary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)]"
              >
                <span className="flex items-center gap-2">
                  <Settings size={13} className="text-[var(--ui-text-tertiary)]" />
                  Settings
                </span>
                <kbd className="rounded bg-[var(--ui-control-fill)] px-1 font-mono text-[9px] text-[var(--ui-text-quaternary)]">Ctrl+,</kbd>
              </button>
              <button
                type="button"
                onClick={() => { setMoreMenuOpen(false); onClearConversation(); }}
                className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[var(--ui-red)] transition-colors duration-100 hover:transition-none hover:bg-[color-mix(in_srgb,var(--ui-red)_14%,transparent)]"
              >
                <Trash2 size={13} />
                <span>Clear conversation</span>
              </button>
            </div>
          )}
        </div>
      </div>
    </header>
  );
};
