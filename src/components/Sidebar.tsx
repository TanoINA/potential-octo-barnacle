import React, { useState } from 'react';
import { Plus, MessageSquare, Folder, Settings, Terminal, Zap, Trash2, Search } from 'lucide-react';
import type { Session, AgentSettings } from '../types/agent';

interface SidebarProps {
  sessions: Session[];
  currentSessionId: string;
  onNewSession: () => void;
  onSelectSession: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onOpenSettings: () => void;
  workspaceCwd: string;
  settings: AgentSettings | null;
}

export const Sidebar: React.FC<SidebarProps> = ({
  sessions,
  currentSessionId,
  onNewSession,
  onSelectSession,
  onDeleteSession,
  onOpenSettings,
  workspaceCwd,
  settings,
}) => {
  const [searchQuery, setSearchQuery] = useState('');
  const activeProvider = settings?.providers.find((p) => p.id === settings.activeProviderId);

  const filteredSessions = sessions.filter((s) =>
    (s.title || '').toLowerCase().includes(searchQuery.toLowerCase())
  );

  return (
    <aside className="w-64 bg-[#0d1117] border-r border-[#30363d] flex flex-col h-full select-none">
      {/* App Header */}
      <div className="p-3.5 border-b border-[#30363d] flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <div className="w-7 h-7 rounded-lg bg-gradient-to-br from-cyan-500 to-blue-600 flex items-center justify-center shadow-lg shadow-cyan-500/20">
            <Zap className="w-4 h-4 text-white fill-white" />
          </div>
          <div>
            <h1 className="font-semibold text-xs tracking-wider text-white uppercase">AetherAgent</h1>
            <div className="flex items-center gap-1.5">
              <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
              <span className="text-[10px] text-slate-400 font-mono">Rust Engine</span>
            </div>
          </div>
        </div>

        <button
          onClick={onNewSession}
          title="New Session (Ctrl+N)"
          className="p-1.5 rounded-md hover:bg-[#21262d] text-slate-300 hover:text-white transition-colors border border-transparent hover:border-[#30363d]"
        >
          <Plus className="w-4 h-4" />
        </button>
      </div>

      {/* New Session Action */}
      <div className="p-3 space-y-2">
        <button
          onClick={onNewSession}
          className="w-full flex items-center justify-center gap-2 py-2 px-3 rounded-lg bg-[#21262d] hover:bg-[#30363d] text-white text-xs font-medium transition-all border border-[#30363d] hover:border-slate-500 shadow-sm"
        >
          <Plus className="w-3.5 h-3.5" />
          <span>New Chat</span>
        </button>

        {/* Search Session Filter */}
        <div className="relative">
          <Search className="w-3.5 h-3.5 text-slate-400 absolute left-2.5 top-2.5 pointer-events-none" />
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search chats..."
            className="w-full bg-[#161b22] border border-[#30363d] rounded-lg pl-8 pr-3 py-1.5 text-[11px] text-slate-200 placeholder-slate-500 focus:outline-none focus:border-cyan-500"
          />
        </div>
      </div>

      {/* Sessions List */}
      <div className="flex-1 overflow-y-auto px-2 space-y-0.5">
        <div className="px-2 py-1 text-[10px] font-semibold tracking-wider text-slate-400 uppercase">
          Recent Conversations
        </div>

        {filteredSessions.length === 0 ? (
          <div className="px-3 py-6 text-center text-xs text-slate-400">
            {searchQuery ? 'No matching chats' : 'No previous chats'}
          </div>
        ) : (
          filteredSessions.map((s) => {
            const isActive = s.id === currentSessionId;
            return (
              <div
                key={s.id}
                className={`group w-full flex items-center justify-between px-2.5 py-2 rounded-lg text-xs transition-colors cursor-pointer ${
                  isActive
                    ? 'bg-[#1f6feb]/20 text-[#58a6ff] font-medium border border-[#388bfd]/30'
                    : 'text-slate-300 hover:bg-[#161b22] hover:text-white'
                }`}
                onClick={() => onSelectSession(s.id)}
              >
                <div className="flex items-center gap-2 truncate pr-1">
                  <MessageSquare className={`w-3.5 h-3.5 shrink-0 ${isActive ? 'text-[#58a6ff]' : 'text-slate-400'}`} />
                  <span className="truncate">{s.title || 'Untitled Session'}</span>
                </div>

                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    if (confirm('Delete this session?')) {
                      onDeleteSession(s.id);
                    }
                  }}
                  className="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-rose-500/20 text-slate-400 hover:text-rose-400 transition-opacity"
                  title="Delete Session"
                >
                  <Trash2 className="w-3 h-3" />
                </button>
              </div>
            );
          })
        )}
      </div>

      {/* Workspace & Settings Footer */}
      <div className="p-3 border-t border-[#30363d] bg-[#0d1117] space-y-2">
        {/* Workspace directory */}
        <div className="flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-[#161b22] border border-[#30363d] text-[11px] text-slate-400">
          <Folder className="w-3.5 h-3.5 shrink-0 text-amber-400" />
          <span className="truncate font-mono" title={workspaceCwd}>
            {workspaceCwd ? workspaceCwd.split(/[\\/]/).pop() || workspaceCwd : 'Workspace'}
          </span>
        </div>

        {/* Active Provider & Settings button */}
        <div className="flex items-center justify-between pt-1">
          <button
            onClick={onOpenSettings}
            className="flex items-center gap-1.5 truncate pr-2 text-left hover:opacity-80 transition-opacity"
            title="Click to change model or provider"
          >
            <Terminal className="w-3.5 h-3.5 text-cyan-400 shrink-0" />
            <div className="truncate">
              <span className="text-[11px] text-slate-200 block truncate font-medium">
                {activeProvider ? activeProvider.name : 'No Provider'}
              </span>
              <span className="text-[10px] text-cyan-400/90 font-mono block truncate">
                {activeProvider ? activeProvider.model : ''}
              </span>
            </div>
          </button>

          <button
            onClick={onOpenSettings}
            className="p-1.5 rounded-md hover:bg-[#21262d] text-slate-400 hover:text-white transition-colors"
            title="Provider & Agent Settings"
          >
            <Settings className="w-4 h-4" />
          </button>
        </div>
      </div>
    </aside>
  );
};
