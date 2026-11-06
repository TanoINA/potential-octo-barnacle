import { useState } from 'react';
import { Bot, CheckCircle2, ChevronDown, ChevronRight, FileCode2, Loader2, XCircle } from 'lucide-react';
import type { SubagentProgress } from '../types/agent';

interface SubagentTreeCardProps {
  subagents: SubagentProgress[];
  onSelectArtifact?: (artifactId: string) => void;
}

export function SubagentTreeCard({ subagents, onSelectArtifact }: SubagentTreeCardProps) {
  const [expanded, setExpanded] = useState(true);

  if (!subagents || subagents.length === 0) return null;

  const runningCount = subagents.filter((s) => s.status === 'running').length;
  const completedCount = subagents.filter((s) => s.status === 'completed').length;

  return (
    <div
      className="mx-auto mb-3 w-full max-w-3xl overflow-hidden rounded-xl border border-[#30363d] bg-[#161b22]/90 shadow-md backdrop-blur-sm transition-all"
      data-testid="subagent-tree-card"
    >
      <header
        onClick={() => setExpanded((prev) => !prev)}
        className="flex cursor-pointer items-center justify-between border-b border-[#30363d]/60 bg-[#161b22] px-3.5 py-2 select-none hover:bg-[#21262d]/70 transition-colors"
      >
        <div className="flex items-center gap-2">
          <Bot size={14} className="text-[#58a6ff]" />
          <span className="text-xs font-semibold text-[#e6edf3]">Subagent Tasks</span>
          <span className="rounded-full bg-[#30363d] px-2 py-0.5 text-[10px] font-mono text-[#8b949e]">
            {completedCount}/{subagents.length} done
          </span>
          {runningCount > 0 && (
            <span className="flex items-center gap-1 rounded-full bg-[#f0883e]/15 px-2 py-0.5 text-[10px] font-medium text-[#f0883e]">
              <Loader2 size={10} className="animate-spin" />
              <span>{runningCount} active</span>
            </span>
          )}
        </div>
        <button
          type="button"
          className="text-[#8b949e] hover:text-[#e6edf3]"
          aria-label={expanded ? 'Collapse subagents' : 'Expand subagents'}
        >
          {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </button>
      </header>

      {expanded && (
        <div className="p-3 space-y-2.5">
          {subagents.map((sub) => {
            const isRunning = sub.status === 'running';
            const isCompleted = sub.status === 'completed';
            const durationMs = (sub.completedAt || Date.now()) - sub.startedAt;
            const durationSec = Math.max(1, Math.round(durationMs / 1000));

            return (
              <div
                key={sub.id}
                className="relative flex items-start gap-3 rounded-lg border border-[#30363d]/50 bg-[#0d1117] p-2.5 transition-colors hover:border-[#30363d]"
              >
                <div className="mt-0.5 shrink-0">
                  {isRunning ? (
                    <Loader2 size={14} className="animate-spin text-[#f0883e]" />
                  ) : isCompleted ? (
                    <CheckCircle2 size={14} className="text-[#3fb950]" />
                  ) : (
                    <XCircle size={14} className="text-[#f85149]" />
                  )}
                </div>

                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span className="text-xs font-semibold text-[#f0f6fc]">{sub.role}</span>
                    <span className="font-mono text-[10px] text-[#8b949e]">{durationSec}s</span>
                  </div>

                  <p className="mt-1 text-[11px] leading-relaxed text-[#8b949e]">{sub.task}</p>

                  {sub.summary && (
                    <div className="mt-1.5 rounded bg-[#161b22] px-2 py-1 text-[10px] text-[#c9d1d9] border border-[#30363d]/40">
                      {sub.summary}
                    </div>
                  )}

                  {sub.artifactId && onSelectArtifact && (
                    <button
                      type="button"
                      onClick={() => onSelectArtifact(sub.artifactId!)}
                      className="mt-2 inline-flex items-center gap-1.5 rounded border border-[#30363d] bg-[#161b22] px-2 py-0.5 text-[10px] font-medium text-[#58a6ff] hover:bg-[#21262d] transition-colors"
                    >
                      <FileCode2 size={11} />
                      <span>View Generated Artifact</span>
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
