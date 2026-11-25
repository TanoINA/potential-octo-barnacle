import React, { useEffect, useRef, useState } from 'react';
import { Check, Copy, Terminal } from 'lucide-react';

interface TerminalOutputProps {
  command?: string;
  output?: string;
  exitCode?: number;
  isRunning?: boolean;
  className?: string;
}

const NEAR_BOTTOM_PX = 24;

export const TerminalOutput: React.FC<TerminalOutputProps> = ({
  command,
  output = '',
  exitCode,
  isRunning = false,
  className = '',
}) => {
  const safeOutput = output || '';
  const scrollRef = useRef<HTMLDivElement>(null);
  const [copied, setCopied] = useState(false);
  const jumpedRef = useRef(false);
  const stickToBottomRef = useRef(true);

  // Jump to bottom initially
  useEffect(() => {
    const el = scrollRef.current;
    if (el && !jumpedRef.current) {
      jumpedRef.current = true;
      el.scrollTop = el.scrollHeight;
    }
  }, []);

  // Smart auto-tail: scroll to bottom only when user is already near bottom
  useEffect(() => {
    const el = scrollRef.current;
    if (el && stickToBottomRef.current) {
      el.scrollTop = el.scrollHeight;
    }
  }, [output]);

  const handleCopy = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      const textToCopy = command ? `$ ${command}\n\n${safeOutput}` : safeOutput;
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(textToCopy);
      } else {
        const textarea = document.createElement('textarea');
        textarea.value = textToCopy;
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand('copy');
        document.body.removeChild(textarea);
      }
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // fallback ignore
    }
  };

  const displayText = safeOutput.trim() ? safeOutput : isRunning ? 'Executing command in PowerShell...' : '(no output / silent exit)';

  return (
    <div className={`my-2 rounded-lg border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-code-bg)] overflow-hidden ${className}`}>
      {/* Terminal Header Bar */}
      <div className="flex items-center justify-between border-b border-[var(--ui-stroke-quaternary)] bg-[var(--ui-widget-bg)] px-3 py-1.5 text-xs">
        <div className="flex items-center gap-2 min-w-0 flex-1 mr-2">
          <Terminal size={13} className="text-[var(--ui-orange)] shrink-0" />
          {command ? (
            <span className="font-mono text-[11px] truncate text-[var(--ui-text-secondary)]" title={command}>
              <span className="text-[var(--ui-orange)] font-semibold mr-1">$</span>
              {command}
            </span>
          ) : (
            <span className="font-mono text-[11px] text-[var(--ui-text-tertiary)]">Terminal Output</span>
          )}
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {/* Status Badge */}
          {isRunning ? (
            <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-mono bg-[var(--ui-orange)]/15 text-[var(--ui-orange)] border border-[var(--ui-orange)]/30 animate-pulse">
              <span className="w-1.5 h-1.5 rounded-full bg-[var(--ui-orange)]" />
              running
            </span>
          ) : exitCode != null ? (
            exitCode === 0 ? (
              <span className="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-mono font-medium bg-[var(--ui-diff-add-bg)] text-[var(--ui-diff-add-fg)] border border-[var(--ui-green)]/35">
                exit 0
              </span>
            ) : (
              <span className="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-mono font-medium bg-[var(--ui-diff-remove-bg)] text-[var(--ui-diff-remove-fg)] border border-[var(--ui-red)]/35">
                exit {exitCode}
              </span>
            )
          ) : null}

          {/* Copy Button */}
          <button
            type="button"
            onClick={handleCopy}
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-[var(--ui-text-tertiary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]"
            title="Copy command and output"
          >
            {copied ? (
              <>
                <Check size={11} className="text-[var(--ui-green)]" />
                <span className="text-[var(--ui-green)]">Copied</span>
              </>
            ) : (
              <>
                <Copy size={11} />
                <span>Copy</span>
              </>
            )}
          </button>
        </div>
      </div>

      {/* Terminal Body */}
      <div
        ref={scrollRef}
        onScroll={(event) => {
          const element = event.currentTarget;
          stickToBottomRef.current = element.scrollHeight - element.scrollTop - element.clientHeight < NEAR_BOTTOM_PX;
        }}
        className="max-h-72 overflow-x-auto overflow-y-auto select-text p-3 font-mono text-[11px] leading-5 text-[var(--ui-text-tertiary)] bg-[var(--ui-code-bg)]"
        data-selectable-text="true"
      >
        <pre className="w-max min-w-full font-mono text-[11px] whitespace-pre text-[var(--ui-text-secondary)]">
          {displayText}
        </pre>
      </div>
    </div>
  );
};
