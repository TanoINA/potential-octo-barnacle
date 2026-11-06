import React, { useMemo } from 'react';
import { Archive, Clock, Download, FileText, Terminal, X } from 'lucide-react';
import type { ChatMessage } from '../types/agent';
import { useModalAccessibility } from '../hooks/useModalAccessibility';

interface SessionHistoryModalProps {
  open: boolean;
  onClose: () => void;
  sessionTitle: string;
  sessionId: string;
  messages: ChatMessage[];
  onCompact: () => void;
  onExport: (format: 'json' | 'markdown' | 'html') => void;
}

export const SessionHistoryModal: React.FC<SessionHistoryModalProps> = ({
  open,
  onClose,
  sessionTitle,
  sessionId,
  messages,
  onCompact,
  onExport,
}) => {
  const dialogRef = useModalAccessibility<HTMLDivElement>(open, onClose);
  const stats = useMemo(() => {
    let userTurns = 0;
    let assistantTurns = 0;
    let toolCallsCount = 0;
    let totalChars = 0;
    const toolsUsed = new Map<string, number>();
    const filesTouched = new Set<string>();

    for (const msg of messages) {
      totalChars += (msg.content?.length || 0) + (msg.reasoning?.length || 0);
      totalChars += (msg.attachments || []).reduce((sum, attachment) => sum + attachment.path.length, 0);
      if (msg.role === 'user') userTurns++;
      if (msg.role === 'assistant') assistantTurns++;

      if (msg.toolCalls) {
        toolCallsCount += msg.toolCalls.length;
        for (const tc of msg.toolCalls) {
          toolsUsed.set(tc.name, (toolsUsed.get(tc.name) || 0) + 1);
          totalChars += JSON.stringify(tc.args ?? {}).length + (tc.result?.length || 0);
          let args: unknown = tc.args;
          if (typeof tc.args === 'string') {
            try {
              args = JSON.parse(tc.args);
            } catch {
              args = null;
            }
          }
          if (args && typeof args === 'object' && 'path' in args) {
            const path = (args as { path?: unknown }).path;
            if (typeof path === 'string') filesTouched.add(path);
          }
          for (const file of tc.filesChanged || []) {
            filesTouched.add(file.path);
          }
        }
      }
    }

    const estimatedTokens = Math.round(totalChars / 4);

    return {
      userTurns,
      assistantTurns,
      toolCallsCount,
      estimatedTokens,
      toolsUsed: Array.from(toolsUsed.entries()),
      filesTouched: Array.from(filesTouched),
    };
  }, [messages]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4"
      onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label="Session history and context"
        tabIndex={-1}
        className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded-2xl border border-[#30363d] bg-[#161b22] text-[#e6edf3] shadow-2xl overflow-hidden"
      >
        {/* Header */}
        <div className="flex h-12 shrink-0 items-center justify-between border-b border-[#30363d] px-4 bg-[#0d1117]">
          <div className="flex items-center gap-2">
            <Clock size={16} className="text-[#58a6ff]" />
            <h2 className="text-xs font-semibold uppercase tracking-wider text-[#e6edf3]">Session History & Context</h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close session history"
            className="flex h-7 w-7 items-center justify-center rounded-md border border-[#30363d] bg-[#161b22] text-[#8b949e] hover:text-white"
          >
            <X size={14} />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4 space-y-4 text-xs">
          {/* Session Overview */}
          <div className="rounded-xl border border-[#30363d] bg-[#0d1117] p-3 space-y-1.5">
            <div className="text-[11px] font-semibold text-[#8b949e]">SESSION TITLE</div>
            <div className="font-mono text-sm text-white font-medium truncate">{sessionTitle || 'New Chat'}</div>
            <div className="text-[10px] text-[#6e7681] font-mono">ID: {sessionId}</div>
          </div>

          {/* Metrics Grid */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
            <div className="rounded-xl border border-[#30363d] bg-[#0d1117] p-3 text-center">
              <div className="text-[10px] text-[#8b949e] uppercase font-semibold">Total Turns</div>
              <div className="mt-1 font-mono text-xl font-bold text-white">{stats.userTurns}</div>
              <div className="text-[9px] text-[#6e7681]">{stats.userTurns} user · {stats.assistantTurns} agent</div>
            </div>
            <div className="rounded-xl border border-[#30363d] bg-[#0d1117] p-3 text-center">
              <div className="text-[10px] text-[#8b949e] uppercase font-semibold">Tool Calls</div>
              <div className="mt-1 font-mono text-xl font-bold text-[#3fb950]">{stats.toolCallsCount}</div>
              <div className="text-[9px] text-[#6e7681]">executed</div>
            </div>
            <div className="rounded-xl border border-[#30363d] bg-[#0d1117] p-3 text-center">
              <div className="text-[10px] text-[#8b949e] uppercase font-semibold">Est. Tokens</div>
              <div className="mt-1 font-mono text-xl font-bold text-[#f0883e]">~{stats.estimatedTokens.toLocaleString()}</div>
              <div className="text-[9px] text-[#6e7681]">stored history estimate</div>
            </div>
            <div className="rounded-xl border border-[#30363d] bg-[#0d1117] p-3 text-center">
              <div className="text-[10px] text-[#8b949e] uppercase font-semibold">Files Touched</div>
              <div className="mt-1 font-mono text-xl font-bold text-[#58a6ff]">{stats.filesTouched.length}</div>
              <div className="text-[9px] text-[#6e7681]">in workspace</div>
            </div>
          </div>

          {/* Files Touched List */}
          {stats.filesTouched.length > 0 && (
            <div className="rounded-xl border border-[#30363d] bg-[#0d1117] p-3">
              <div className="text-[10px] font-semibold uppercase tracking-wider text-[#8b949e] mb-2 flex items-center gap-1.5">
                <FileText size={12} className="text-[#58a6ff]" />
                Files Touched in this Session
              </div>
              <div className="max-h-36 overflow-y-auto space-y-1 font-mono text-[11px]">
                {stats.filesTouched.map((f) => (
                  <div key={f} className="truncate rounded px-2 py-1 bg-[#161b22] text-[#c9d1d9] border border-[#30363d]/50">
                    {f}
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Tools Distribution */}
          {stats.toolsUsed.length > 0 && (
            <div className="rounded-xl border border-[#30363d] bg-[#0d1117] p-3">
              <div className="text-[10px] font-semibold uppercase tracking-wider text-[#8b949e] mb-2 flex items-center gap-1.5">
                <Terminal size={12} className="text-[#3fb950]" />
                Tool Executions Breakdown
              </div>
              <div className="flex flex-wrap gap-1.5">
                {stats.toolsUsed.map(([name, count]) => (
                  <span key={name} className="inline-flex items-center gap-1 rounded-md border border-[#30363d] bg-[#161b22] px-2 py-1 text-[11px] font-mono text-[#c9d1d9]">
                    <span className="text-[#3fb950]">{name}</span>
                    <span className="rounded bg-[#30363d] px-1 text-[9px] text-[#8b949e]">{count}</span>
                  </span>
                ))}
              </div>
            </div>
          )}
        </div>

        {/* Footer Actions */}
        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-[#30363d] bg-[#0d1117] px-4 py-3">
          <button
            type="button"
            onClick={() => {
              onClose();
              onCompact();
            }}
            disabled={messages.length < 3}
            className="flex items-center gap-1.5 rounded-lg border border-[#f0883e]/50 bg-[#f0883e]/15 px-3 py-1.5 text-xs font-medium text-[#f0883e] hover:bg-[#f0883e]/25 disabled:opacity-40 transition-colors"
          >
            <Archive size={13} />
            Compact Context (Free Tokens)
          </button>

          <div className="flex items-center gap-1.5">
            <button
              type="button"
              onClick={() => { onClose(); onExport('html'); }}
              className="flex items-center gap-1 rounded-md border border-[#30363d] bg-[#161b22] px-2.5 py-1.5 text-xs text-[#8b949e] hover:text-white transition-colors"
            >
              <Download size={12} /> HTML
            </button>
            <button
              type="button"
              onClick={() => { onClose(); onExport('markdown'); }}
              className="flex items-center gap-1 rounded-md border border-[#30363d] bg-[#161b22] px-2.5 py-1.5 text-xs text-[#8b949e] hover:text-white transition-colors"
            >
              <Download size={12} /> Markdown
            </button>
            <button
              type="button"
              onClick={() => { onClose(); onExport('json'); }}
              className="flex items-center gap-1 rounded-md border border-[#30363d] bg-[#161b22] px-2.5 py-1.5 text-xs text-[#8b949e] hover:text-white transition-colors"
            >
              <Download size={12} /> JSON
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
