import { memo, useCallback, useState } from 'react';
import {
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  Folder,
  Info,
  Paperclip,
  Pencil,
  RotateCcw,
  Undo2,
} from 'lucide-react';
import type { ChatMessage } from '../types/agent';
import { MarkdownRenderer } from './MarkdownRenderer';
import { ExecutionTrace } from './ExecutionTrace';
import { FilesChangedCard } from './FilesChangedCard';
import { CouncilView } from './CouncilView';
import { SelfImprovementBadge } from './SelfImprovementBadge';

interface MessageRowProps {
  message: ChatMessage;
  onSelectArtifact: (id: string) => void;
  onSelectCouncilWinner?: (messageId: string, winner: 'A' | 'B') => void;
  onRetry?: () => void;
  onEditAndResend?: (newContent: string) => void;
  onRollback?: () => void;
  isStreaming?: boolean;
}

export const MessageRow = memo(function MessageRow({
  message,
  onSelectArtifact,
  onSelectCouncilWinner,
  onRetry,
  onEditAndResend,
  onRollback,
  isStreaming = false,
}: MessageRowProps) {
  const [reasoningCollapsed, setReasoningCollapsed] = useState(true);
  const [isEditing, setIsEditing] = useState(false);
  const [editText, setEditText] = useState(message?.content || '');
  const [copied, setCopied] = useState(false);

  if (!message) return null;

  const isUser = message.role === 'user';
  const isSystem = message.role === 'system';

  const handleCopy = useCallback(() => {
    if (!message.content) return;
    void navigator.clipboard.writeText(message.content).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  }, [message.content]);

  const handleSaveEdit = useCallback(() => {
    const trimmed = (editText || '').trim();
    if (!trimmed || isStreaming || !onEditAndResend) return;
    setIsEditing(false);
    onEditAndResend(trimmed);
  }, [editText, isStreaming, onEditAndResend]);

  const handleCancelEdit = useCallback(() => {
    setIsEditing(false);
    setEditText(message.content || '');
  }, [message.content]);

  if (isSystem) {
    return (
      <div className="mx-auto my-3 flex w-full max-w-3xl flex-col">
        <div className="rounded-xl border border-[#30363d] bg-[#0d1117] p-3 text-xs shadow-sm">
          <div className="flex items-center justify-between mb-2 pb-1.5 border-b border-[#30363d]/60">
            <span className="font-mono text-[11px] font-semibold text-[#58a6ff] uppercase tracking-wider flex items-center gap-1.5">
              <Info size={12} className="text-[#f0883e]" /> System Context Checkpoint
            </span>
            <time className="text-[10px] text-[#6e7681]">
              {new Date(message.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
            </time>
          </div>
          <div className="text-[#8b949e] select-text font-normal leading-relaxed">
            <MarkdownRenderer content={message.content} />
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className={`group mx-auto flex w-full max-w-3xl flex-col pb-5 ${isUser ? 'items-end' : 'items-start'}`}>
      <div className="mb-1.5 flex items-center gap-2 px-1">
        <span className={`text-[11px] font-medium ${isUser ? 'text-[#58a6ff]' : 'text-[#8b949e]'}`}>
          {isUser ? 'You' : 'AetherAgent'}
        </span>
        <time className="text-[10px] text-[#6e7681]">
          {new Date(message.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
        </time>
        {message.isStreaming && <span className="text-[10px] text-[#58a6ff] animate-pulse">streaming</span>}
      </div>

      {isUser && message.attachments && message.attachments.length > 0 && (
        <div className="mb-2 flex flex-wrap justify-end gap-1.5 max-w-full">
          {message.attachments.map((att) => (
            <span
              key={att.id}
              className="inline-flex items-center gap-1 rounded-md border border-[#30363d] bg-[#161b22] px-2 py-0.5 text-[11px] font-mono text-[#58a6ff]"
            >
              {att.isDir ? <Folder size={11} className="text-[#d29922]" /> : <Paperclip size={11} className="text-[#58a6ff]" />}
              <span className="truncate max-w-[220px]" title={att.path}>{att.name}</span>
            </span>
          ))}
        </div>
      )}

      {message.reasoning && (
        <div className="mb-2 w-full overflow-hidden rounded-lg border border-[#30363d] bg-[#0d1117]">
          <button
            type="button"
            onClick={() => setReasoningCollapsed((value) => !value)}
            className="flex w-full items-center justify-between px-3 py-2 text-left text-xs text-[#a5d6ff] hover:bg-[#161b22]"
            aria-expanded={!reasoningCollapsed}
          >
            <span className="flex items-center gap-2">
              {reasoningCollapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
              Thinking process
            </span>
            <span className="text-[10px] text-[#6e7681]">{message.reasoning.length} chars</span>
          </button>
          {!reasoningCollapsed && (
            <div className="max-h-64 select-text overflow-y-auto whitespace-pre-wrap border-t border-[#30363d] px-3 py-2 font-mono text-[11px] leading-5 text-[#8b949e]">
              {message.reasoning}
            </div>
          )}
        </div>
      )}

      {/* Execution Trace (Hermes style progress / status accordion) */}
      {!isUser && message.toolCalls && message.toolCalls.length > 0 && (
        <ExecutionTrace
          toolCalls={message.toolCalls}
          isStreaming={message.isStreaming}
          onSelectArtifact={onSelectArtifact}
        />
      )}

      {/* Council Multi-Model Comparison if available */}
      {!isUser && message.councilTurn && (
        <CouncilView
          councilTurn={message.councilTurn}
          onSelectWinner={(winner) => onSelectCouncilWinner?.(message.id, winner)}
        />
      )}

      {/* Bubble Content or Inline Editor */}
      {isUser && isEditing ? (
        <div className="w-full rounded-xl border border-[#30363d] bg-[#0d1117] p-3 text-xs shadow-md">
          <textarea
            value={editText}
            onChange={(e) => setEditText(e.target.value)}
            className="w-full resize-none rounded-lg border border-[#30363d] bg-[#161b22] p-2.5 text-xs text-[#e6edf3] placeholder:text-[#6e7681] focus:border-[#58a6ff] focus:outline-none"
            rows={Math.max(2, Math.min(8, (editText || '').split('\n').length + 1))}
            autoFocus
            onKeyDown={(e) => {
              if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
                e.preventDefault();
                handleSaveEdit();
              } else if (e.key === 'Escape') {
                handleCancelEdit();
              }
            }}
          />
          <div className="mt-2 flex items-center justify-between text-[11px] text-[#8b949e]">
            <span>Press <kbd className="rounded border border-[#30363d] bg-[#161b22] px-1 font-mono text-[10px]">Ctrl+Enter</kbd> to submit</span>
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={handleCancelEdit}
                className="rounded px-2.5 py-1 text-xs text-[#8b949e] hover:text-[#e6edf3] hover:bg-[#21262d] transition-colors"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={!(editText || '').trim() || isStreaming}
                onClick={handleSaveEdit}
                className="rounded bg-[#238636] px-3 py-1 text-xs font-medium text-white hover:bg-[#2ea043] disabled:opacity-40 transition-colors"
              >
                Save & Submit
              </button>
            </div>
          </div>
        </div>
      ) : (
        message.content && (
          <div
            className={`max-w-full select-text break-words rounded-xl px-4 py-3 text-[13px] leading-6 ${
              isUser
                ? 'bg-[#1f6feb] text-white whitespace-pre-wrap'
                : 'border border-[#30363d] bg-[#161b22] text-[#e6edf3]'
            }`}
          >
            {isUser ? message.content : <MarkdownRenderer content={message.content} />}
          </div>
        )
      )}

      {!message.content && message.isStreaming && !message.reasoning && (!message.toolCalls || message.toolCalls.length === 0) && (
        <div className="rounded-lg border border-[#30363d] bg-[#161b22] px-3 py-2 text-xs text-[#8b949e]">Working…</div>
      )}

      {/* Action Bar (Copy, Edit, Retry, Rollback) */}
      {!isEditing && !message.isStreaming && (
        <div
          className={`mt-1 flex items-center gap-1 text-[11px] text-[#6e7681] opacity-60 group-hover:opacity-100 transition-opacity ${
            isUser ? 'justify-end' : 'justify-start'
          }`}
        >
          {/* Copy Message */}
          {message.content && (
            <button
              type="button"
              onClick={handleCopy}
              title="Copy message to clipboard"
              aria-label="Copy message"
              className="flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-[#21262d] hover:text-[#e6edf3] transition-colors"
            >
              {copied ? <Check size={11} className="text-[#3fb950]" /> : <Copy size={11} />}
              <span className={`text-[10px] ${copied ? 'text-[#3fb950]' : ''}`}>
                {copied ? 'Copied' : 'Copy'}
              </span>
            </button>
          )}

          {/* Edit (User only) */}
          {isUser && onEditAndResend && (
            <button
              type="button"
              onClick={() => {
                setEditText(message.content || '');
                setIsEditing(true);
              }}
              title="Edit message & resend from here"
              aria-label="Edit message"
              className="flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-[#21262d] hover:text-[#e6edf3] transition-colors"
            >
              <Pencil size={11} />
              <span className="text-[10px]">Edit</span>
            </button>
          )}

          {/* Retry / Regenerate */}
          {onRetry && (
            <button
              type="button"
              onClick={onRetry}
              title={isUser ? 'Retry prompt from here' : 'Regenerate this response'}
              aria-label={isUser ? 'Retry prompt' : 'Regenerate response'}
              className="flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-[#21262d] hover:text-[#58a6ff] transition-colors"
            >
              <RotateCcw size={11} />
              <span className="text-[10px]">{isUser ? 'Retry' : 'Regenerate'}</span>
            </button>
          )}

          {/* Rollback to here */}
          {onRollback && (
            <button
              type="button"
              onClick={onRollback}
              title="Rollback conversation to this turn"
              aria-label="Rollback to this message"
              className="flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-[#21262d] hover:text-[#f85149] transition-colors"
            >
              <Undo2 size={11} />
              <span className="text-[10px]">Rollback</span>
            </button>
          )}
        </div>
      )}

      {!isUser && message.selfImprovement && (
        <SelfImprovementBadge review={message.selfImprovement} />
      )}

      {/* Files Changed Card (Hermes style bottom diff summary) */}
      {!isUser && message.toolCalls && message.toolCalls.length > 0 && (
        <FilesChangedCard
          toolCalls={message.toolCalls}
          onSelectArtifact={onSelectArtifact}
        />
      )}
    </div>
  );
});
