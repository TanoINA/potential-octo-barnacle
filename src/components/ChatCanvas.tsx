import React, { useState, useRef, useEffect } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Send, Square, ChevronDown, ChevronRight, FileCode, CheckCircle2, Clock, AlertCircle } from 'lucide-react';
import type { ChatMessage, ToolCallItem } from '../types/agent';

interface ChatCanvasProps {
  messages: ChatMessage[];
  isStreaming: boolean;
  onSendMessage: (text: string) => void;
  onStop: () => void;
  onSelectArtifact: (id: string) => void;
}

export const ChatCanvas: React.FC<ChatCanvasProps> = ({
  messages,
  isStreaming,
  onSendMessage,
  onStop,
  onSelectArtifact,
}) => {
  const [input, setInput] = useState('');
  const [collapsedReasoning, setCollapsedReasoning] = useState<Record<string, boolean>>({});

  const parentRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

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

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleSend = () => {
    if (!input.trim() || isStreaming) return;
    onSendMessage(input);
    setInput('');
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  };

  const toggleReasoning = (msgId: string) => {
    setCollapsedReasoning((prev) => ({
      ...prev,
      [msgId]: !prev[msgId],
    }));
  };

  return (
    <div className="flex-1 flex flex-col h-full bg-[#0b0f17] relative">
      {/* Messages Scroll Area (Virtualized) */}
      <div ref={parentRef} className="flex-1 overflow-y-auto px-4 py-6">
        {messages.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center text-center max-w-md mx-auto">
            <div className="w-12 h-12 rounded-2xl bg-gradient-to-tr from-cyan-500 to-blue-600 flex items-center justify-center shadow-xl shadow-cyan-500/10 mb-4">
              <span className="text-xl font-bold text-white">Æ</span>
            </div>
            <h2 className="text-lg font-semibold text-white mb-2">Welcome to AetherAgent</h2>
            <p className="text-xs text-slate-400 mb-6 leading-relaxed">
              Native, ultra-fast agent powered by Rust & Tauri 2.0. Connect to your custom provider, inspect codebases, execute terminal commands, and create artifacts without lag.
            </p>
            <div className="grid grid-cols-2 gap-2 w-full text-left">
              <div className="p-2.5 rounded-lg bg-[#161b22] border border-[#30363d] text-xs text-slate-300">
                <span className="font-semibold text-cyan-400 block mb-1">⚡ 60 FPS Native</span>
                Virtualized chat list with zero DOM stutter.
              </div>
              <div className="p-2.5 rounded-lg bg-[#161b22] border border-[#30363d] text-xs text-slate-300">
                <span className="font-semibold text-emerald-400 block mb-1">🌐 Any Provider</span>
                Works with DeepSeek, Kimi, OpenAI, Ollama.
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
                      <span className="text-[10px] text-slate-400 font-mono">
                        {new Date(msg.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                      </span>
                    </div>

                    {/* Thinking Accordion (DeepSeek R1 / Kimi) */}
                    {msg.reasoning && (
                      <div className="w-full mb-3 rounded-lg bg-[#161b22]/70 border border-[#30363d] overflow-hidden text-xs">
                        <button
                          onClick={() => toggleReasoning(msg.id)}
                          className="w-full flex items-center gap-2 px-3 py-2 bg-[#161b22] hover:bg-[#21262d] text-slate-400 hover:text-slate-200 transition-colors"
                        >
                          {isCollapsed ? <ChevronRight className="w-3.5 h-3.5" /> : <ChevronDown className="w-3.5 h-3.5" />}
                          <span className="font-mono text-[11px] font-medium text-purple-400">Thinking Process</span>
                        </button>
                        {!isCollapsed && (
                          <div className="px-3 py-2 text-slate-400 font-mono text-[11px] whitespace-pre-wrap leading-relaxed border-t border-[#30363d]/50 bg-[#0d1117]/50 max-h-60 overflow-y-auto">
                            {msg.reasoning}
                          </div>
                        )}
                      </div>
                    )}

                    {/* Main Content Bubble */}
                    {msg.content && (
                      <div
                        className={`rounded-xl px-4 py-3 text-xs leading-relaxed max-w-full whitespace-pre-wrap select-text ${
                          isUser
                            ? 'bg-[#1f6feb] text-white shadow-md'
                            : 'bg-[#161b22] border border-[#30363d] text-slate-200 shadow-sm'
                        }`}
                      >
                        {msg.content}
                      </div>
                    )}

                    {/* Compact Tool Status Pills */}
                    {msg.toolCalls && msg.toolCalls.length > 0 && (
                      <div className="w-full mt-2 space-y-1.5">
                        {msg.toolCalls.map((t: ToolCallItem) => {
                          const isDone = t.status === 'done';
                          const isErr = t.status === 'error';
                          return (
                            <div
                              key={t.id}
                              className="flex items-center justify-between px-3 py-1.5 rounded-lg bg-[#161b22] border border-[#30363d] text-[11px]"
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
                                  className="flex items-center gap-1.5 px-2 py-0.5 rounded bg-[#21262d] hover:bg-[#30363d] text-[#58a6ff] hover:text-white font-medium text-[10px] transition-colors border border-[#30363d]"
                                >
                                  <FileCode className="w-3 h-3" />
                                  <span>Open Sidecar</span>
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

      {/* Composer Input Box */}
      <div className="p-4 border-t border-[#30363d] bg-[#0d1117]">
        <div className="max-w-3xl mx-auto relative rounded-xl bg-[#161b22] border border-[#30363d] focus-within:border-cyan-500/80 focus-within:ring-1 focus-within:ring-cyan-500/50 transition-all shadow-lg">
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
            placeholder="Ask AetherAgent to code, inspect files, or run commands... (Enter to send, Shift+Enter for new line)"
            className="w-full bg-transparent text-xs text-white placeholder-slate-400 px-4 pt-3 pb-10 resize-none focus:outline-none"
          />

          <div className="absolute right-3 bottom-2.5 flex items-center gap-2">
            {isStreaming ? (
              <button
                onClick={onStop}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-rose-600 hover:bg-rose-500 text-white text-xs font-medium transition-colors shadow-sm"
              >
                <Square className="w-3.5 h-3.5 fill-white" />
                <span>Stop</span>
              </button>
            ) : (
              <button
                onClick={handleSend}
                disabled={!input.trim()}
                className="p-2 rounded-lg bg-cyan-600 hover:bg-cyan-500 disabled:opacity-40 disabled:hover:bg-cyan-600 text-white transition-all shadow-md shadow-cyan-600/20"
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
