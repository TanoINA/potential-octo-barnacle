import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Check,
  BookOpen,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Copy,
  Folder,
  Image as ImageIcon,
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
import { InteractiveQuestionCard, type QuestionItem } from './InteractiveQuestionCard';
import type { ToolCallItem } from '../types/agent';

interface MessageRowProps {
  message: ChatMessage;
  onSelectArtifact: (id: string) => void;
  onSelectCouncilWinner?: (messageId: string, winner: 'A' | 'B') => void;
  onRetry?: () => void;
  onEditAndResend?: (newContent: string) => void;
  onRollback?: () => void;
  onRetryMessage?: (messageId: string) => void;
  onEditMessage?: (messageId: string, newContent: string) => void;
  onRollbackMessage?: (messageId: string) => void;
  onAnswerQuestion?: (answer: string) => void;
  isStreaming?: boolean;
}

export const MessageRow = memo(function MessageRow({
  message,
  onSelectArtifact,
  onSelectCouncilWinner,
  onRetry,
  onEditAndResend,
  onRollback,
  onRetryMessage,
  onEditMessage,
  onRollbackMessage,
  onAnswerQuestion,
  isStreaming = false,
}: MessageRowProps) {
  const [reasoningCollapsed, setReasoningCollapsed] = useState(true);
  const [isEditing, setIsEditing] = useState(false);
  const [editText, setEditText] = useState(message?.content || '');
  const [copied, setCopied] = useState(false);
  const [isPromptExpanded, setIsPromptExpanded] = useState(false);

  const isUser = message?.role === 'user';

  const questionToolCalls = useMemo(() => {
    if (isUser || !message) return [];
    const calls: { id?: string; questions: QuestionItem[] }[] = [];
    // The same tool call lives in message.toolCalls and its segment, so dedupe by id.
    const seen = new Set<string>();
    const checkTool = (tc: ToolCallItem) => {
      if (tc.name === 'ask_question' && !(tc.id && seen.has(tc.id))) {
        const rawQuestions = tc.args?.questions || [];
        if (Array.isArray(rawQuestions) && rawQuestions.length > 0) {
          if (tc.id) seen.add(tc.id);
          calls.push({ id: tc.id, questions: rawQuestions as QuestionItem[] });
        }
      }
    };
    if (message.toolCalls) message.toolCalls.forEach(checkTool);
    if (message.segments) {
      for (const seg of message.segments) {
        if (seg.toolCalls) seg.toolCalls.forEach(checkTool);
      }
    }
    return calls;
  }, [isUser, message?.toolCalls, message?.segments]);

  const copyTimerRef = useRef<number | null>(null);
  useEffect(() => () => {
    if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
  }, []);

  const handleCopy = useCallback(() => {
    if (!message) return;
    let fullText = '';
    if (message.segments && message.segments.length > 0) {
      fullText = message.segments.map((s) => s.text).filter(Boolean).join('\n\n');
    }
    if (!fullText) {
      fullText = [message.preamble, message.content].filter(Boolean).join('\n\n');
    }
    if (!fullText) return;
    void navigator.clipboard?.writeText(fullText).then(() => {
      setCopied(true);
      if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
      copyTimerRef.current = window.setTimeout(() => setCopied(false), 2000);
    }).catch(() => undefined);
  }, [message]);

  const handleSaveEdit = useCallback(() => {
    if (!message) return;
    const trimmed = (editText || '').trim();
    if (!trimmed || isStreaming || (!onEditAndResend && !onEditMessage)) return;
    setIsEditing(false);
    if (onEditMessage) onEditMessage(message.id, trimmed);
    else onEditAndResend?.(trimmed);
  }, [editText, isStreaming, message, onEditAndResend, onEditMessage]);

  const handleCancelEdit = useCallback(() => {
    setIsEditing(false);
    setEditText(message?.content || '');
  }, [message?.content]);

  if (!message) return null;
  const isSystem = message.role === 'system';
  const isLongPrompt = isUser && ((message.content?.length || 0) > 320 || (message.content?.split('\n').length || 0) > 6);
  const hasRenderableSegments = Boolean(message.segments?.some((segment) => segment.text?.trim() || segment.toolCalls?.length));

  if (isSystem) {
    return (
      <div className="mx-auto my-3 flex w-full max-w-3xl flex-col">
        <div className="rounded-xl border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-widget-bg)] p-3 text-xs">
          <div className="mb-2 flex items-center justify-between border-b border-[var(--ui-stroke-quaternary)] pb-1.5">
            <span className="flex items-center gap-1.5 font-mono text-[11px] font-semibold uppercase tracking-wider text-[var(--ui-accent)]">
              <Info size={12} className="text-[var(--ui-orange)]" /> System Context Checkpoint
            </span>
            <time className="text-[10px] text-[var(--ui-text-quaternary)]">
              {new Date(message.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
            </time>
          </div>
          <div className="select-text font-normal leading-relaxed text-[var(--ui-text-tertiary)]">
            <MarkdownRenderer content={message.content} />
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      data-message-id={message.id}
      data-message-role={message.role}
      className={`group mx-auto flex w-full max-w-3xl flex-col pb-5 ${isUser ? 'items-end' : 'items-start'}`}
    >
      <div className="mb-1.5 flex items-center gap-2 px-1">
        <span className={`text-[11px] font-medium ${isUser ? 'text-[var(--ui-accent)]' : 'text-[var(--ui-text-tertiary)]'}`}>
          {isUser ? 'You' : 'AetherAgent'}
        </span>
        <time className="text-[10px] text-[var(--ui-text-quaternary)]">
          {new Date(message.timestamp).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
        </time>
        {message.isStreaming && !message.notice && <span className="text-[10px] text-[var(--ui-accent)] animate-pulse">streaming</span>}
        {message.isStreaming && message.notice && <span className="text-[10px] text-[var(--ui-yellow)] animate-pulse">reconnecting</span>}
      </div>

      {isUser && Boolean(message.selectedSkills?.length) && (
        <div aria-label="Skills used for this message" className="mb-2 flex max-w-full flex-wrap justify-end gap-1.5">
          {message.selectedSkills?.map((name) => <span key={name} className="inline-flex max-w-full items-center gap-1.5 rounded-md border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] px-2 py-0.5 text-[11px] text-[var(--ui-accent)]">
            <BookOpen size={11} className="shrink-0" /><span className="max-w-56 truncate">{name}</span>
          </span>)}
        </div>
      )}

      {isUser && message.attachments && message.attachments.length > 0 && (
        <div className="mb-2 flex flex-wrap justify-end gap-1.5 max-w-full">
          {message.attachments.map((att) => (
            <span
              key={att.id}
              className="inline-flex items-center gap-1 rounded-md border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] px-2 py-0.5 text-[11px] font-mono text-[var(--ui-accent)]"
            >
              {att.isDir ? (
                <Folder size={11} className="text-[var(--ui-yellow)]" />
              ) : /\.(png|jpe?g|gif|webp|svg|bmp)$/i.test(att.name) ? (
                <ImageIcon size={11} className="text-[var(--ui-purple)]" />
              ) : (
                <Paperclip size={11} className="text-[var(--ui-accent)]" />
              )}
              <span className="truncate max-w-[220px]" title={att.path}>{att.name}</span>
            </span>
          ))}
        </div>
      )}

      {message.reasoning && (
        <div className="mb-2 w-full overflow-hidden rounded-lg border border-[var(--ui-stroke-quaternary)] bg-[var(--ui-widget-bg)]">
          <button
            type="button"
            onClick={() => setReasoningCollapsed((value) => !value)}
            className="flex w-full items-center justify-between px-3 py-2 text-left text-xs text-[var(--conversation-scaffold-text)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]"
            aria-expanded={!reasoningCollapsed}
          >
            <span className="flex items-center gap-2">
              {reasoningCollapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
              Thinking process
            </span>
            <span className="text-[10px] text-[var(--conversation-scaffold-meta)]">{message.reasoning.length} chars</span>
          </button>
          {!reasoningCollapsed && (
            <div className="max-h-64 select-text overflow-y-auto whitespace-pre-wrap border-t border-[var(--ui-stroke-quaternary)] px-3 py-2 font-mono text-[11px] leading-5 text-[var(--conversation-scaffold-text)]">
              {message.reasoning}
            </div>
          )}
        </div>
      )}

      {/* Interleaved Segments (per-turn flow: text -> tools -> text -> tools) */}
      {!isUser && hasRenderableSegments ? (
        <div className="w-full space-y-3">
          {message.segments?.map((seg, idx) => {
            const hasText = Boolean(seg.text && seg.text.trim().length > 0);
            const hasTools = Boolean(seg.toolCalls && seg.toolCalls.length > 0);
            if (!hasText && !hasTools) return null;

            return (
              <div key={seg.id || idx} className="w-full space-y-2">
                {hasText && (
                  <div
                    className="max-w-full select-text break-words px-0.5 py-0.5 text-[13.5px] leading-[1.65] text-[var(--ui-text-primary)]"
                    data-testid={idx === 0 ? "message-preamble" : undefined}
                  >
                    <MarkdownRenderer content={seg.text!} />
                  </div>
                )}

                {hasTools && (
                  <ExecutionTrace
                    toolCalls={seg.toolCalls!}
                    isStreaming={message.isStreaming && idx === message.segments!.length - 1}
                    onSelectArtifact={onSelectArtifact}
                  />
                )}
              </div>
            );
          })}
        </div>
      ) : (
        <>
          {/* Opening Preamble (rendered above ExecutionTrace) */}
          {!isUser && message.preamble && (
            <div
              className="mb-2 max-w-full select-text break-words px-0.5 py-0.5 text-[13.5px] leading-[1.65] text-[var(--ui-text-primary)]"
              data-testid="message-preamble"
            >
              <MarkdownRenderer content={message.preamble} />
            </div>
          )}

          {/* Execution Trace (progress / status accordion) */}
          {!isUser && message.toolCalls && message.toolCalls.length > 0 && (
            <ExecutionTrace
              toolCalls={message.toolCalls}
              isStreaming={message.isStreaming}
              onSelectArtifact={onSelectArtifact}
            />
          )}
        </>
      )}

      {/* Council Multi-Model Comparison if available */}
      {!isUser && message.councilTurn && (
        <CouncilView
          councilTurn={message.councilTurn}
          isStreaming={Boolean(message.isStreaming)}
          onSelectWinner={onSelectCouncilWinner ? (winner) => onSelectCouncilWinner(message.id, winner) : undefined}
        />
      )}

      {/* Bubble Content or Inline Editor */}
      {isUser && isEditing ? (
        <div className="w-full rounded-xl border border-[var(--ui-stroke-secondary)] bg-[var(--ui-bg-elevated)] p-3 text-xs shadow-nous">
          <textarea
            value={editText}
            onChange={(e) => setEditText(e.target.value)}
            className="w-full resize-none rounded-lg border border-[var(--ui-stroke-secondary)] bg-[var(--ui-bg-editor)] p-2.5 text-xs text-[var(--ui-text-primary)] placeholder:text-[var(--ui-text-quaternary)] focus:border-[var(--ui-accent)] focus:outline-none"
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
          <div className="mt-2 flex items-center justify-between text-[11px] text-[var(--ui-text-tertiary)]">
            <span>Press <kbd className="rounded border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] px-1 font-mono text-[10px]">Ctrl+Enter</kbd> to submit</span>
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={handleCancelEdit}
                className="rounded px-2.5 py-1 text-xs text-[var(--ui-text-tertiary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={!(editText || '').trim() || isStreaming}
                onClick={handleSaveEdit}
                className="rounded bg-[var(--ui-accent)] px-3 py-1 text-xs font-medium text-[var(--ui-accent-fg)] transition-opacity hover:opacity-90 disabled:opacity-40"
              >
                Save & Submit
              </button>
            </div>
          </div>
        </div>
      ) : (
        isUser ? (
          message.content && (
            <div className="max-w-[85%] select-text break-words rounded-[11px] border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-user-bubble)] text-[var(--ui-text-primary)]">
              <div className={`px-3.5 py-2.5 text-[13.5px] leading-relaxed whitespace-pre-wrap ${isLongPrompt && !isPromptExpanded ? 'max-h-36 overflow-hidden relative' : ''}`}>
                {message.content}
                {isLongPrompt && !isPromptExpanded && (
                  <div className="pointer-events-none absolute inset-x-0 bottom-0 h-12 bg-gradient-to-t from-[var(--ui-user-bubble)] to-transparent" />
                )}
              </div>
              {isLongPrompt && (
                <button
                  type="button"
                  onClick={() => setIsPromptExpanded((prev) => !prev)}
                  className="flex w-full items-center justify-center gap-1 rounded-b-[11px] border-t border-[var(--ui-stroke-tertiary)] px-3 py-1.5 text-[11px] font-medium text-[var(--ui-text-secondary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]"
                  aria-expanded={isPromptExpanded}
                >
                  {isPromptExpanded ? (
                    <>
                      <ChevronUp size={12} />
                      <span>Show less</span>
                    </>
                  ) : (
                    <>
                      <ChevronDown size={12} />
                      <span>Show more</span>
                    </>
                  )}
                </button>
              )}
            </div>
          )
        ) : !hasRenderableSegments ? (
          message.content && (
            <div className="max-w-full select-text break-words px-0.5 py-0.5 text-[13.5px] leading-[1.65] text-[var(--ui-text-primary)]">
              <MarkdownRenderer content={message.content} />
            </div>
          )
        ) : null
      )}

      {!message.content && !message.preamble && message.isStreaming && !message.reasoning && (!message.toolCalls || message.toolCalls.length === 0) && (
        <div className="rounded-lg bg-[var(--ui-widget-bg)] px-3 py-2 text-xs text-[var(--ui-text-tertiary)]">Working…</div>
      )}

      {message.notice && (
        <div
          data-testid="provider-notice"
          className="mt-2 inline-flex items-center gap-2 rounded-md border border-[var(--ui-yellow)]/35 bg-[var(--ui-yellow)]/10 px-2.5 py-1 text-xs text-[var(--ui-yellow)] animate-fadeIn"
        >
          <span className="relative flex h-2 w-2">
            <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[var(--ui-yellow)] opacity-75" />
            <span className="relative inline-flex rounded-full h-2 w-2 bg-[var(--ui-yellow)]" />
          </span>
          <span className="font-mono text-[11px] font-medium">{message.notice.message}</span>
        </div>
      )}

      {/* Action Bar (Copy, Edit, Retry, Rollback) */}
      {!isEditing && !message.isStreaming && (
        <div
          className={`mt-1 flex items-center gap-1 text-[11px] text-[var(--ui-text-quaternary)] opacity-60 group-hover:opacity-100 transition-opacity ${
            isUser ? 'justify-end' : 'justify-start'
          }`}
        >
          {/* Copy Message */}
          {(message.content || message.preamble) && (
            <button
              type="button"
              onClick={handleCopy}
              title="Copy message to clipboard"
              aria-label="Copy message"
              className="flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]"
            >
              {copied ? <Check size={11} className="text-[var(--ui-green)]" /> : <Copy size={11} />}
              <span className={`text-[10px] ${copied ? 'text-[var(--ui-green)]' : ''}`}>
                {copied ? 'Copied' : 'Copy'}
              </span>
            </button>
          )}

          {/* Edit (User only) */}
          {isUser && (onEditAndResend || onEditMessage) && (
            <button
              type="button"
              onClick={() => {
                setEditText(message.content || '');
                setIsEditing(true);
              }}
              title="Edit message & resend from here"
              aria-label="Edit message"
              className="flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]"
            >
              <Pencil size={11} />
              <span className="text-[10px]">Edit</span>
            </button>
          )}

          {/* Retry / Regenerate */}
          {(onRetry || onRetryMessage) && (
            <button
              type="button"
              onClick={() => {
                if (onRetryMessage) onRetryMessage(message.id);
                else onRetry?.();
              }}
              title={isUser ? 'Retry prompt from here' : 'Regenerate this response'}
              aria-label={isUser ? 'Retry prompt' : 'Regenerate response'}
              className="flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-accent)]"
            >
              <RotateCcw size={11} />
              <span className="text-[10px]">{isUser ? 'Retry' : 'Regenerate'}</span>
            </button>
          )}

          {/* Rollback to here */}
          {(onRollback || onRollbackMessage) && (
            <button
              type="button"
              onClick={() => {
                if (onRollbackMessage) onRollbackMessage(message.id);
                else onRollback?.();
              }}
              title="Rollback conversation to this turn"
              aria-label="Rollback to this message"
              className="flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-red)]"
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

      {/* Files Changed Card (bottom diff summary) */}
      {!isUser && message.toolCalls && message.toolCalls.length > 0 && (
        <FilesChangedCard
          toolCalls={message.toolCalls}
          onSelectArtifact={onSelectArtifact}
        />
      )}

      {/* Interactive Question Cards */}
      {!isUser && questionToolCalls.map((item, index) => (
        <InteractiveQuestionCard
          key={item.id || index}
          questions={item.questions}
          onSelectOption={onAnswerQuestion}
        />
      ))}
    </div>
  );
});
