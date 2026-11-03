import React, { useState, useRef, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Send, Square, ChevronDown, ChevronRight, FileCode, CheckCircle2, Clock, AlertCircle, Sparkles, Trash2, Command } from 'lucide-react';
import type { ChatMessage, ToolCallItem, AgentSettings } from '../types/agent';

interface ChatCanvasProps {
  messages: ChatMessage[];
  isStreaming: boolean;
  onSendMessage: (text: string) => void;
  onStop: () => void;
  onSelectArtifact: (id: string) => void;
  onClearSession: () => void;
  onOpenSettings: () => void;
  settings: AgentSettings | null;
  sessionTitle: string;
}

export const ChatCanvas: React.FC<ChatCanvasProps> = ({
  messages,
  isStreaming,
  onSendMessage,
  onStop,
  onSelectArtifact,
  onClearSession,
  onOpenSettings,
  settings,
  sessionTitle,
}) => {
  const [input, setInput] = useState('');
  const [collapsedReasoning, setCollapsedReasoning] = useState<Record<string, boolean>>({});
  const [showSlashMenu, setShowSlashMenu] = useState(false);

  const parentRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const activeProvider = settings?.providers.find((p) => p.id === settings.activeProviderId);

  // Virtualizer for smooth 60 FPS scrolling
  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 100,
    overscan: 5,
  });

  // Auto-scroll to bottom on new message or streaming
  useEffect(() => {
    if (messages.length > 0 && parentRef.current) {
      parentRef.current.scrollTop = parentRef.current.scrollHeight;
    }
  }, [messages, isStreaming]);

  // Handle slash menu triggers
  useEffect(() => {
    if (input.startsWith('/')) {
      setShowSlashMenu(true);
    } else {
      setShowSlashMenu(false);
    }
  }, [input]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleSend = () => {
    if (!input.trim() || isStreaming) return;
    const command = input.trim().toLowerCase();

    // Built-in slash commands
    if (command === '/clear') {
      onClearSession();
      setInput('');
      return;
    }
    if (command === '/settings' || command === '/model') {
      onOpenSettings();
      setInput('');
      return;
    }

    onSendMessage(input);
    setInput('');
    setShowSlashMenu(false);
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  };

  const selectSlashCommand = (cmd: string) => {
    if (cmd === '/clear') {
      onClearSession();
      setInput('');
    } else if (cmd === '/settings' || cmd === '/model') {
      onOpenSettings();
      setInput('');
    } else {
      setInput(cmd + ' ');
    }
    setShowSlashMenu(false);
  };

  const toggleReasoning = (msgId: string) => {
    setCollapsedReasoning((prev) => ({
      ...prev,
      [msgId]: !prev[msgId],
    }));
  };

  return (
    <div className="flex-1 flex flex-col h-full bg-[#0b0f17] relative">
      {/* Top Bar (Antigravity Style) */}
      <header className="h-11 border-b border-[#30363d] bg-[#0d1117] px-4 flex items-center justify-between z-10 select-none">
        <div className="flex items-center gap-2 truncate">
          <span className="text-xs font-semibold text-slate-200 truncate">{sessionTitle || 'New Chat'}</span>
          <span className="text-[10px] text-slate-500 font-mono hidden sm:inline">({messages.length} msgs)</span>
        </div>

        <div className="flex items-center gap-2">
          {/* Quick Model Pill */}
          <button
            onClick={onOpenSettings}
            className="flex items-center gap-1.5 px-2.5 py-1 rounded-full bg-[#161b22] hover:bg-[#21262d] border border-[#30363d] text-cyan-400 text-[11px] font-mono transition-colors shadow-sm"
            title="Active Model (Click to change)"
          >
            <Sparkles className="w-3 h-3 text-cyan-400" />
            <span className="truncate max-w-[140px] font-medium">
              {activeProvider?.model || 'No model'}
            </span>
          </button>

          {/* Clear Chat Button */}
          {messages.length > 0 && (
            <button
              onClick={onClearSession}
              className="p-1.5 rounded-md hover:bg-[#21262d] text-slate-400 hover:text-rose-400 transition-colors"
              title="Clear messages in this chat"
            >
              <Trash2 className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </header>

      {/* Messages Scroll Area (Virtualized 60 FPS) */}
      <div ref={parentRef} className="flex-1 overflow-y-auto px-4 py-6">
        {messages.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-center max-w-lg mx-auto py-12">
            <div className="w-14 h-14 rounded-2xl bg-gradient-to-tr from-cyan-500 to-blue-600 flex items-center justify-center shadow-xl shadow-cyan-500/20 mb-4">
              <span className="text-2xl font-bold text-white">Æ</span>
            </div>
            <h2 className="text-lg font-bold text-white mb-2">AetherAgent</h2>
            <p className="text-xs text-slate-400 mb-6 leading-relaxed">
              Native, ultra-fast AI coding agent with custom provider support. Type your instruction or use slash commands like <code className="text-cyan-400 bg-[#161b22] px-1 py-0.5 rounded">/clear</code> and <code className="text-cyan-400 bg-[#161b22] px-1 py-0.5 rounded">/model</code>.
            </p>

            <div className="grid grid-cols-2 gap-2.5 w-full text-left">
              <div className="p-3 rounded-xl bg-[#161b22] border border-[#30363d] text-xs text-slate-300">
                <span className="font-semibold text-cyan-400 block mb-1">⚡ 60 FPS Native</span>
                Virtualized windowing keeps memory under 40 MB.
              </div>
              <div className="p-3 rounded-xl bg-[#161b22] border border-[#30363d] text-xs text-slate-300">
                <span className="font-semibold text-emerald-400 block mb-1">🌐 Any Provider</span>
                Auto-search models from GutsAI, DeepSeek, or Ollama.
              </div>
              <div className="p-3 rounded-xl bg-[#161b22] border border-[#30363d] text-xs text-slate-300">
                <span className="font-semibold text-purple-400 block mb-1">🧠 Thinking Process</span>
                Collapsible reasoning view for DeepSeek R1 & Kimi.
              </div>
              <div className="p-3 rounded-xl bg-[#161b22] border border-[#30363d] text-xs text-slate-300">
                <span className="font-semibold text-amber-400 block mb-1">📦 Artifact Sidecar</span>
                Large file reads and command logs never bloat chat.
              </div>
            </div>
          </div>
        ) : (
          <div
            style={{
              height: `${virtualizer.getTotalSize()}px`,
              width: '100%',
              position: 'relative',
            }}
          >
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const msg = messages[virtualRow.index];
              const isUser = msg.role === 'user';
              const isCollapsed = collapsedReasoning[msg.id];

              return (
                <div
                  key={msg.id}
                  data-index={virtualRow.index}
                  ref={virtualizer.measureElement}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                  className="pb-6"
                >
                  <div className={`flex flex-col ${isUser ? 'items-end' : 'items-start'} max-w-3xl mx-auto`}>
                    {/* Role header */}
                    <div className="flex items-center gap-2 mb-1.5 px-1">
                      <span className={`text-[11px] font-medium ${isUser ? 'text-cyan-400' : 'text-slate-400'}`}>
                        {isUser ? 'You' : 'AetherAgent'}
                      </span>
                      <span className="text-[10px] text-slate-500 font-mono">
                        {new Date(msg.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                      </span>
                    </div>

                    {/* Thinking Process Card (DeepSeek R1 / Kimi) */}
                    {msg.reasoning && (
                      <div className="w-full mb-3 rounded-xl bg-[#161b22]/70 border border-[#30363d] overflow-hidden text-xs shadow-sm">
                        <button
                          onClick={() => toggleReasoning(msg.id)}
                          className="w-full flex items-center justify-between px-3.5 py-2 bg-[#161b22] hover:bg-[#21262d] text-slate-400 hover:text-slate-200 transition-colors"
                        >
                          <div className="flex items-center gap-2">
                            {isCollapsed ? <ChevronRight className="w-3.5 h-3.5 text-purple-400" /> : <ChevronDown className="w-3.5 h-3.5 text-purple-400" />}
                            <span className="font-mono text-[11px] font-medium text-purple-300">Thinking Process</span>
                          </div>
                          <span className="text-[10px] text-slate-500 font-mono">{msg.reasoning.length} chars</span>
                        </button>
                        {!isCollapsed && (
                          <div className="px-3.5 py-2.5 text-slate-400 font-mono text-[11px] whitespace-pre-wrap leading-relaxed border-t border-[#30363d]/50 bg-[#0d1117]/60 max-h-64 overflow-y-auto">
                            {msg.reasoning}
                          </div>
                        )}
                      </div>
                    )}

                    {/* Main Message Bubble */}
                    {msg.content && (
                      <div
                        className={`rounded-2xl px-4 py-3 text-xs leading-relaxed max-w-full whitespace-pre-wrap select-text ${
                          isUser
                            ? 'bg-[#1f6feb] text-white shadow-md'
                            : 'bg-[#161b22] border border-[#30363d] text-slate-200 shadow-sm'
                        }`}
                      >
                        {msg.content}
                      </div>
                    )}

                    {/* Compact Tool Status Badges */}
                    {msg.toolCalls && msg.toolCalls.length > 0 && (
                      <div className="w-full mt-2 space-y-1.5">
                        {msg.toolCalls.map((t: ToolCallItem) => {
                          const isDone = t.status === 'done';
                          const isErr = t.status === 'error';
                          return (
                            <div
                              key={t.id}
                              className="flex items-center justify-between px-3.5 py-2 rounded-xl bg-[#161b22] border border-[#30363d] text-[11px] shadow-sm"
                            >
                              <div className="flex items-center gap-2 truncate pr-2">
                                {isDone ? (
                                  <CheckCircle2 className="w-3.5 h-3.5 text-emerald-400 shrink-0" />
                                ) : isErr ? (
                                  <AlertCircle className="w-3.5 h-3.5 text-rose-400 shrink-0" />
                                ) : (
                                  <Clock className="w-3.5 h-3.5 text-amber-400 animate-spin shrink-0" />
                                )}
                                <span className="font-mono font-medium text-slate-300 truncate">
                                  {t.name}
                                  {t.args?.path ? `: ${t.args.path}` : t.args?.command ? `: ${t.args.command}` : ''}
                                </span>
                              </div>

                              {t.artifactId && (
                                <button
                                  onClick={() => onSelectArtifact(t.artifactId!)}
                                  className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-[#21262d] hover:bg-[#30363d] text-[#58a6ff] hover:text-white font-medium text-[10px] transition-colors border border-[#30363d]"
                                >
                                  <FileCode className="w-3 h-3" />
                                  <span>View Sidecar</span>
                                </button>
                              )}
                            </div>
                          );
                        })}
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Slash Commands Dropdown Menu */}
      {showSlashMenu && (
        <div className="absolute left-1/2 -translate-x-1/2 bottom-20 w-full max-w-md bg-[#161b22] border border-[#30363d] rounded-xl shadow-2xl p-1.5 z-20 space-y-1">
          <div className="px-2 py-1 text-[10px] font-semibold tracking-wider text-slate-400 uppercase flex items-center gap-1">
            <Command className="w-3 h-3" />
            <span>Slash Commands</span>
          </div>
          <button
            onClick={() => selectSlashCommand('/clear')}
            className="w-full flex items-center justify-between px-3 py-1.5 rounded-lg text-left text-xs text-slate-300 hover:bg-[#21262d] hover:text-white"
          >
            <span className="font-mono text-cyan-400">/clear</span>
            <span className="text-[11px] text-slate-400">Clear chat history</span>
          </button>
          <button
            onClick={() => selectSlashCommand('/model')}
            className="w-full flex items-center justify-between px-3 py-1.5 rounded-lg text-left text-xs text-slate-300 hover:bg-[#21262d] hover:text-white"
          >
            <span className="font-mono text-cyan-400">/model</span>
            <span className="text-[11px] text-slate-400">Switch model / provider</span>
          </button>
          <button
            onClick={() => selectSlashCommand('/settings')}
            className="w-full flex items-center justify-between px-3 py-1.5 rounded-lg text-left text-xs text-slate-300 hover:bg-[#21262d] hover:text-white"
          >
            <span className="font-mono text-cyan-400">/settings</span>
            <span className="text-[11px] text-slate-400">Open Settings modal</span>
          </button>
        </div>
      )}

      {/* Composer Input Box */}
      <div className="p-4 border-t border-[#30363d] bg-[#0d1117]">
        <div className="max-w-3xl mx-auto relative rounded-2xl bg-[#161b22] border border-[#30363d] focus-within:border-cyan-500/80 focus-within:ring-1 focus-within:ring-cyan-500/50 transition-all shadow-xl">
          <textarea
            ref={textareaRef}
            rows={2}
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              e.target.style.height = 'auto';
              e.target.style.height = `${Math.min(e.target.scrollHeight, 180)}px`;
            }}
            onKeyDown={handleKeyDown}
            placeholder="Type your message or / for commands... (Enter to send, Shift+Enter for newline)"
            className="w-full bg-transparent text-xs text-white placeholder-slate-500 px-4 pt-3 pb-10 resize-none focus:outline-none"
          />

          <div className="absolute right-3 bottom-2.5 flex items-center gap-2">
            {isStreaming ? (
              <button
                onClick={onStop}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-rose-600 hover:bg-rose-500 text-white text-xs font-medium transition-colors shadow-sm"
              >
                <Square className="w-3.5 h-3.5 fill-white" />
                <span>Stop</span>
              </button>
            ) : (
              <button
                onClick={handleSend}
                disabled={!input.trim()}
                className="p-2 rounded-xl bg-cyan-600 hover:bg-cyan-500 disabled:opacity-40 disabled:hover:bg-cyan-600 text-white transition-all shadow-md shadow-cyan-600/20"
              >
                <Send className="w-3.5 h-3.5" />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
