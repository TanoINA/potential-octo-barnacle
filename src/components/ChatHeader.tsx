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
    <header className="z-20 flex h-11 shrink-0 items-center justify-between border-b border-[#30363d] bg-[#0d1117] px-2 sm:px-3">
      <div className="flex items-center gap-1.5 min-w-0 mr-1.5">
        {onToggleSidebar && (
          <button
            type="button"
            onClick={onToggleSidebar}
            className="flex h-8 w-8 items-center justify-center rounded-lg border border-[#30363d] bg-[#161b22] text-[#8b949e] hover:text-[#e6edf3] transition-colors shrink-0"
            title={sidebarOpen ? 'Hide sidebar (Ctrl+B)' : 'Show sidebar (Ctrl+B)'}
          >
            <PanelLeft size={13} />
          </button>
        )}
        <div className="min-w-0 max-w-[56px] sm:max-w-[180px] truncate">
          <h1 className="truncate text-xs font-semibold text-[#e6edf3]">{sessionTitle || 'New Chat'}</h1>
          {totalMessageCount > 0 && <span className="text-[12px] text-[#8b949e] whitespace-nowrap block">{totalMessageCount} messages</span>}
        </div>
      </div>

      {/* Action toolbar */}
      <div className="flex items-center gap-1 sm:gap-1.5 shrink-0">
        {onOpenLiveShell && (
          <button
            type="button"
            onClick={onOpenLiveShell}
            className="flex h-8 w-8 items-center justify-center rounded-lg border border-[#30363d] bg-[#161b22] text-[#8b949e] hover:border-[#8b949e] hover:text-[#e6edf3] transition-colors shrink-0"
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
          className={`flex h-8 items-center gap-1 rounded-lg border px-2 text-xs transition-colors shrink-0 disabled:cursor-not-allowed disabled:opacity-40 ${
            councilMode
              ? 'border-[#8b949e] bg-[#21262d] text-[#e6edf3] font-medium'
              : 'border-[#30363d] bg-[#161b22] text-[#8b949e] hover:text-[#e6edf3]'
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
              className={`flex h-8 items-center gap-1.5 rounded-lg border px-2.5 text-xs transition-colors shrink-0 ${
                isArtifactOpen
                  ? 'border-[#8b949e] bg-[#21262d] text-[#e6edf3] font-medium'
                  : runningTasks > 0
                  ? 'border-[#30363d] bg-[#161b22] text-[#d29922] font-medium hover:bg-[#21262d]'
                  : 'border-[#30363d] bg-[#161b22] text-[#8b949e] hover:text-[#e6edf3]'
              } disabled:cursor-not-allowed disabled:opacity-40 hover:disabled:text-[#8b949e]`}
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
                    <span className="relative inline-flex rounded-full h-2 w-2 bg-[#f0883e]" />
                  </span>
                  <span className="text-[11px] font-mono font-semibold text-[#d29922]">{runningTasks} Active</span>
                </>
              ) : (
                <>
                  <PanelRight size={13} />
                  {artifactCount > 0 && (
                    <span className="rounded bg-[#30363d] px-1 font-mono text-[10px] font-medium text-[#e6edf3]">
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
            className={`flex h-8 w-8 items-center justify-center rounded-lg border text-xs transition-colors ${
              moreMenuOpen
                ? 'border-[#8b949e] bg-[#161b22] text-[#e6edf3]'
                : 'border-[#30363d] bg-[#161b22] text-[#8b949e] hover:text-[#e6edf3]'
            }`}
            title="More actions (Search, Export, Settings)"
          >
            <MoreHorizontal size={14} />
          </button>
          {moreMenuOpen && (
            <div className="ui-menu absolute right-0 top-full mt-1.5 z-50 w-52 rounded-xl border border-[#30363d] bg-[#161b22] p-1 shadow-lg">
              {onOpenSpotlight && (
                <button
                  type="button"
                  onClick={() => { setMoreMenuOpen(false); onOpenSpotlight(); }}
                  className="flex w-full items-center justify-between rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                >
                  <span className="flex items-center gap-2">
                    <Search size={13} className="text-[#8b949e]" />
                    Search
                  </span>
                  <kbd className="rounded border border-[#30363d] bg-[#0d1117] px-1 text-[9px] text-[#6e7681]">Alt+Space</kbd>
                </button>
              )}
              <button
                type="button"
                onClick={() => {
                  setMoreMenuOpen(false);
                  onOpenInChatSearch();
                }}
                className="flex w-full items-center justify-between rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
              >
                <span className="flex items-center gap-2">
                  <Search size={13} className="text-[#8b949e]" />
                  Find in chat
                </span>
                <kbd className="rounded border border-[#30363d] bg-[#0d1117] px-1 text-[9px] text-[#6e7681]">Ctrl+F</kbd>
              </button>
              {onExportSession && (
                <>
                  <div className="my-1 border-t border-[#30363d]/60 px-2 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-[#6e7681]">Export</div>
                  <button
                    type="button"
                    onClick={() => { setMoreMenuOpen(false); onExportSession('html'); }}
                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                  >
                    <Download size={13} className="text-[#8b949e]" />
                    <span>HTML (PDF-ready)</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => { setMoreMenuOpen(false); onExportSession('markdown'); }}
                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                  >
                    <Download size={13} className="text-[#8b949e]" />
                    <span>Markdown (.md)</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => { setMoreMenuOpen(false); onExportSession('json'); }}
                    className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                  >
                    <Download size={13} className="text-[#8b949e]" />
                    <span>JSON transcript</span>
                  </button>
                </>
              )}
              <div className="my-1 border-t border-[#30363d]/60" />
              <button
                type="button"
                onClick={() => { setMoreMenuOpen(false); onOpenSettings(); }}
                className="flex w-full items-center justify-between rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
              >
                <span className="flex items-center gap-2">
                  <Settings size={13} className="text-[#8b949e]" />
                  Settings
                </span>
                <kbd className="rounded border border-[#30363d] bg-[#0d1117] px-1 text-[9px] text-[#6e7681]">Ctrl+,</kbd>
              </button>
              <button
                type="button"
                onClick={() => { setMoreMenuOpen(false); onClearConversation(); }}
                className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[#f85149] hover:bg-[#f85149]/15"
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
