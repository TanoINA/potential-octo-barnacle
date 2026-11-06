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
    if (el && el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM_PX) {
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
    <div className={`my-2 rounded-lg border border-[#30363d] bg-[#0a0d12] overflow-hidden shadow-md ${className}`}>
      {/* Terminal Header Bar */}
      <div className="flex items-center justify-between border-b border-[#21262d] bg-[#161b22] px-3 py-1.5 text-xs">
        <div className="flex items-center gap-2 min-w-0 flex-1 mr-2">
          <Terminal size={13} className="text-[#f0883e] shrink-0" />
          {command ? (
            <span className="font-mono text-[11px] truncate text-[#c9d1d9]" title={command}>
              <span className="text-[#f0883e] font-semibold mr-1">$</span>
              {command}
            </span>
          ) : (
            <span className="font-mono text-[11px] text-[#8b949e]">Terminal Output</span>
          )}
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {/* Status Badge */}
          {isRunning ? (
            <span className="inline-flex items-center gap-1 px-1.5 py-0.5 rounded text-[10px] font-mono bg-[#382618] text-[#f0883e] border border-[#f0883e]/30 animate-pulse">
              <span className="w-1.5 h-1.5 rounded-full bg-[#f0883e]" />
              running
            </span>
          ) : exitCode !== undefined ? (
            exitCode === 0 ? (
              <span className="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-mono font-medium bg-[#238636]/20 text-[#3fb950] border border-[#238636]/40">
                exit 0
              </span>
            ) : (
              <span className="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-mono font-medium bg-[#da3633]/20 text-[#f85149] border border-[#da3633]/40">
                exit {exitCode}
              </span>
            )
          ) : null}

          {/* Copy Button */}
          <button
            type="button"
            onClick={handleCopy}
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-[#8b949e] hover:text-[#e6edf3] hover:bg-[#30363d] transition-colors"
            title="Copy command and output"
          >
            {copied ? (
              <>
                <Check size={11} className="text-[#3fb950]" />
                <span className="text-[#3fb950]">Copied</span>
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
        className="max-h-72 overflow-x-auto overflow-y-auto select-text p-3 font-mono text-[11px] leading-5 text-[#8b949e] bg-[#0a0d12]"
        data-selectable-text="true"
      >
        <pre className="w-max min-w-full font-mono text-[11px] whitespace-pre text-[#c9d1d9]">
          {displayText}
        </pre>
      </div>
    </div>
  );
};
