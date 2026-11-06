import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Check, Copy, CornerDownLeft, Loader2, Terminal, Trash2 } from 'lucide-react';

interface ShellHistoryEntry {
  id: string;
  command: string;
  stdout: string;
  stderr: string;
  exitCode: number;
  durationMs: number;
  timestamp: number;
}

interface ShellCommandResult {
  stdout: string;
  stderr: string;
  exitCode: number;
  durationMs: number;
}

export const InteractiveTerminal: React.FC = () => {
  const [history, setHistory] = useState<ShellHistoryEntry[]>([]);
  const [input, setInput] = useState('');
  const [commandHistory, setCommandHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState<number | null>(null);
  const [isRunning, setIsRunning] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [history, isRunning]);

  const handleSubmit = async (e?: React.FormEvent) => {
    if (e) e.preventDefault();
    const cmd = input.trim();
    if (!cmd || isRunning) return;

    if (cmd.toLowerCase() === 'clear' || cmd.toLowerCase() === 'cls') {
      setHistory([]);
      setInput('');
      return;
    }

    setCommandHistory((prev) => [...prev, cmd]);
    setHistoryIndex(null);
    setInput('');
    setIsRunning(true);

    try {
      const res = await invoke<ShellCommandResult>('run_shell_command', { command: cmd });
      const entry: ShellHistoryEntry = {
        id: `cmd-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        command: cmd,
        stdout: res.stdout,
        stderr: res.stderr,
        exitCode: res.exitCode,
        durationMs: res.durationMs,
        timestamp: Date.now(),
      };
      setHistory((prev) => [...prev, entry]);
    } catch (err) {
      const entry: ShellHistoryEntry = {
        id: `cmd-${Date.now()}`,
        command: cmd,
        stdout: '',
        stderr: String(err),
        exitCode: 1,
        durationMs: 0,
        timestamp: Date.now(),
      };
      setHistory((prev) => [...prev, entry]);
    } finally {
      setIsRunning(false);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (commandHistory.length === 0) return;
      const nextIdx = historyIndex === null ? commandHistory.length - 1 : Math.max(0, historyIndex - 1);
      setHistoryIndex(nextIdx);
      setInput(commandHistory[nextIdx]);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (historyIndex === null) return;
      if (historyIndex < commandHistory.length - 1) {
        const nextIdx = historyIndex + 1;
        setHistoryIndex(nextIdx);
        setInput(commandHistory[nextIdx]);
      } else {
        setHistoryIndex(null);
        setInput('');
      }
    } else if (e.key === 'l' && e.ctrlKey) {
      e.preventDefault();
      setHistory([]);
    }
  };

  const copyEntry = async (id: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedId(id);
      setTimeout(() => setCopiedId(null), 1500);
    } catch {
      // ignore
    }
  };

  return (
    <div className="flex h-full flex-col bg-[#0d1117] font-mono text-[11px] text-[#e6edf3]">
      {/* Top Toolbar */}
      <div className="flex shrink-0 items-center justify-between border-b border-[#30363d] bg-[#161b22] px-3 py-1.5 text-xs">
        <div className="flex items-center gap-2">
          <Terminal size={13} className="text-[#58a6ff]" />
          <span className="font-semibold text-[#e6edf3]">Live Shell</span>
          <span className="text-[10px] text-[#8b949e]">(bounded workspace command)</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => setHistory([])}
            className="flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-[#8b949e] hover:bg-[#21262d] hover:text-[#f85149] transition-colors"
            title="Clear terminal history (Ctrl+L)"
          >
            <Trash2 size={11} />
            <span>Clear</span>
          </button>
        </div>
      </div>

      {/* Output Log */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto p-3 space-y-3 select-text">
        {history.length === 0 && !isRunning && (
          <div className="py-8 text-center text-[#6e7681]">
            <p className="text-xs">Type any command below (e.g. <code className="text-[#79c0ff]">dir</code>, <code className="text-[#79c0ff]">git status</code>, <code className="text-[#79c0ff]">npm test</code>)</p>
            <p className="text-[10px] mt-1 text-[#484f58]">Runs in your active workspace environment</p>
          </div>
        )}

        {history.map((entry) => (
          <div key={entry.id} className="rounded-lg border border-[#21262d] bg-[#161b22]/50 p-2.5 space-y-1.5 group">
            {/* Command Header */}
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-1.5 font-semibold text-[#58a6ff]">
                <span className="text-[#8b949e] select-none">$</span>
                <span>{entry.command}</span>
              </div>
              <div className="flex items-center gap-2">
                <span
                  className={`px-1.5 py-0.2 rounded text-[9px] font-bold ${
                    entry.exitCode === 0
                      ? 'bg-[#238636]/20 text-[#3fb950] border border-[#238636]/30'
                      : 'bg-[#da3633]/20 text-[#f85149] border border-[#da3633]/30'
                  }`}
                >
                  exit {entry.exitCode}
                </span>
                <span className="text-[9px] text-[#6e7681]">{entry.durationMs}ms</span>
                <button
                  type="button"
                  onClick={() => copyEntry(entry.id, entry.stdout || entry.stderr)}
                  className="opacity-0 group-hover:opacity-100 text-[#8b949e] hover:text-white transition-opacity"
                  title="Copy output"
                >
                  {copiedId === entry.id ? <Check size={11} className="text-[#3fb950]" /> : <Copy size={11} />}
                </button>
              </div>
            </div>

            {/* stdout / stderr */}
            {entry.stdout && (
              <pre className="whitespace-pre-wrap break-words text-[#c9d1d9] leading-relaxed text-[11px] overflow-x-auto">
                {entry.stdout}
              </pre>
            )}
            {entry.stderr && (
              <pre className="whitespace-pre-wrap break-words text-[#ff7b72] leading-relaxed text-[11px] overflow-x-auto bg-[#da3633]/10 p-1.5 rounded">
                {entry.stderr}
              </pre>
            )}
          </div>
        ))}

        {isRunning && (
          <div className="flex items-center gap-2 text-xs text-[#58a6ff] animate-pulse py-1">
            <Loader2 size={13} className="animate-spin" />
            <span>Running command...</span>
          </div>
        )}
      </div>

      {/* Input Prompt Bar */}
      <form onSubmit={handleSubmit} className="shrink-0 border-t border-[#30363d] bg-[#161b22] p-2 flex items-center gap-2">
        <span className="text-[#58a6ff] select-none font-bold pl-1">&gt;</span>
        <input
          ref={inputRef}
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Type command and press Enter..."
          disabled={isRunning}
          className="min-w-0 flex-1 bg-transparent text-xs text-[#e6edf3] outline-none placeholder:text-[#6e7681]"
        />
        <button
          type="submit"
          disabled={!input.trim() || isRunning}
          className="flex items-center gap-1 rounded bg-[#238636] px-2.5 py-1 text-[11px] font-medium text-white hover:bg-[#2ea043] disabled:opacity-40 transition-colors"
        >
          {isRunning ? <Loader2 size={11} className="animate-spin" /> : <CornerDownLeft size={11} />}
          <span>Run</span>
        </button>
      </form>
    </div>
  );
};
