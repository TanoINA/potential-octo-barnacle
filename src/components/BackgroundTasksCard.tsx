import { useState } from 'react';
import { Ban, CheckCircle2, ChevronDown, ChevronRight, Copy, Loader2, RotateCw, Square, Terminal, XCircle } from 'lucide-react';
import type { BackgroundTask } from '../types/agent';

interface BackgroundTasksCardProps {
  tasks: BackgroundTask[];
  onKillTask?: (taskId: string) => void;
  onRefreshTasks?: () => void;
}

export function BackgroundTasksCard({ tasks, onKillTask, onRefreshTasks }: BackgroundTasksCardProps) {
  const [expanded, setExpanded] = useState(false);
  const [openLogs, setOpenLogs] = useState<Record<string, boolean>>({});
  const [copiedId, setCopiedId] = useState<string | null>(null);

  if (!tasks || tasks.length === 0) return null;

  const runningCount = tasks.filter((t) => t.status === 'running').length;
  const completedCount = tasks.filter((t) => t.status === 'completed').length;
  const failedCount = tasks.filter((t) => t.status === 'failed').length;
  const killedCount = tasks.filter((t) => t.status === 'killed').length;

  const toggleLog = (id: string) => {
    setOpenLogs((prev) => ({ ...prev, [id]: !prev[id] }));
  };

  const copyOutput = (id: string, text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 1500);
  };

  return (
    <div
      className="mx-auto mb-3 w-full max-w-3xl overflow-hidden rounded-xl border border-[#30363d] bg-[#161b22]/90 shadow-md backdrop-blur-sm transition-all"
      data-testid="background-tasks-card"
    >
      <header
        onClick={() => setExpanded((prev) => !prev)}
        className="flex cursor-pointer items-center justify-between border-b border-[#30363d]/60 bg-[#161b22] px-3.5 py-2 select-none hover:bg-[#21262d]/70 transition-colors"
      >
        <div className="flex items-center gap-2">
          <Terminal size={14} className="text-[#58a6ff]" />
          <span className="text-xs font-semibold text-[#e6edf3]">Background Tasks</span>
          <span className="rounded-full bg-[#30363d] px-2 py-0.5 text-[10px] font-mono text-[#8b949e]">
            {completedCount}/{tasks.length} done
            {failedCount > 0 && ` · ${failedCount} failed`}
            {killedCount > 0 && ` · ${killedCount} killed`}
          </span>
          {runningCount > 0 && (
            <span className="flex items-center gap-1 rounded-full bg-[#f0883e]/15 px-2 py-0.5 text-[10px] font-medium text-[#f0883e]">
              <Loader2 size={10} className="animate-spin" />
              <span>{runningCount} active</span>
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {onRefreshTasks && (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onRefreshTasks();
              }}
              className="text-[#8b949e] hover:text-[#e6edf3] p-1 rounded hover:bg-[#30363d]/50 transition-colors"
              title="Refresh background tasks"
              aria-label="Refresh tasks"
            >
              <RotateCw size={12} />
            </button>
          )}
          <button
            type="button"
            className="text-[#8b949e] hover:text-[#e6edf3]"
            aria-label={expanded ? 'Collapse tasks' : 'Expand tasks'}
          >
            {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          </button>
        </div>
      </header>

      {expanded && (
        <div className="max-h-[min(400px,50vh)] overflow-y-auto overscroll-contain p-3 space-y-2.5 select-text">
          {tasks.map((task) => {
            const isRunning = task.status === 'running';
            const isCompleted = task.status === 'completed';
            const isKilled = task.status === 'killed';
            const durationMs = (task.completedAt || Date.now()) - task.startedAt;
            const durationSec = Math.max(1, Math.round(durationMs / 1000));
            const isLogOpen = !!openLogs[task.id];

            return (
              <div
                key={task.id}
                className="relative flex flex-col gap-2 rounded-lg border border-[#30363d]/50 bg-[#0d1117] p-2.5 transition-colors hover:border-[#30363d]"
              >
                <div className="flex items-start justify-between gap-2">
                  <div className="flex items-center gap-2 min-w-0">
                    <div className="shrink-0">
                      {isRunning ? (
                        <Loader2 size={14} className="animate-spin text-[#f0883e]" />
                      ) : isCompleted ? (
                        <CheckCircle2 size={14} className="text-[#3fb950]" />
                      ) : isKilled ? (
                        <Ban size={14} className="text-[#8b949e]" />
                      ) : (
                        <XCircle size={14} className="text-[#f85149]" />
                      )}
                    </div>
                    <span className="text-xs font-semibold text-[#f0f6fc] truncate">
                      {task.toolSummary || task.command}
                    </span>
                    <span className="rounded bg-[#21262d] px-1.5 py-0.2 text-[10px] font-mono text-[#8b949e]">
                      {task.id}
                    </span>
                    {task.pid && (
                      <span className="rounded bg-[#21262d] px-1.5 py-0.2 text-[10px] font-mono text-[#8b949e]">
                        PID {task.pid}
                      </span>
                    )}
                  </div>

                  <div className="flex items-center gap-2 shrink-0">
                    <span className="font-mono text-[10px] text-[#8b949e]">
                      {durationSec}s
                    </span>
                    {isRunning && onKillTask && (
                      <button
                        type="button"
                        onClick={() => onKillTask(task.id)}
                        className="inline-flex items-center gap-1 rounded border border-[#f85149]/40 bg-[#f85149]/10 px-2 py-0.5 text-[10px] font-medium text-[#f85149] hover:bg-[#f85149]/20 transition-colors"
                        title="Kill background task"
                      >
                        <Square size={9} className="fill-current" />
                        <span>Kill</span>
                      </button>
                    )}
                  </div>
                </div>

                <div className="rounded bg-[#161b22] px-2 py-1 font-mono text-[10px] text-[#8b949e] overflow-x-auto whitespace-pre">
                  {task.command}
                </div>

                <div className="flex items-center justify-between pt-1">
                  <button
                    type="button"
                    onClick={() => toggleLog(task.id)}
                    className="text-[10px] font-medium text-[#58a6ff] hover:underline"
                  >
                    {isLogOpen ? 'Hide Logs' : 'View Logs'}
                  </button>
                  <span className="text-[10px] font-mono text-[#8b949e] truncate max-w-[280px]" title={task.logPath}>
                    {task.logPath}
                  </span>
                </div>

                {isLogOpen && (
                  <div className="relative mt-1">
                    <div className="max-h-48 overflow-y-auto rounded bg-[#090d13] p-2 font-mono text-[10px] leading-relaxed text-[#c9d1d9] border border-[#30363d]/60 whitespace-pre-wrap select-text">
                      {task.recentOutput || '(no output recorded yet)'}
                    </div>
                    {task.recentOutput && (
                      <button
                        type="button"
                        onClick={() => copyOutput(task.id, task.recentOutput)}
                        className="absolute right-2 top-2 rounded bg-[#21262d] p-1 text-[#8b949e] hover:text-[#e6edf3] transition-colors"
                        title="Copy logs"
                      >
                        <Copy size={11} />
                        {copiedId === task.id && (
                          <span className="ml-1 text-[9px] text-[#3fb950]">Copied</span>
                        )}
                      </button>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
