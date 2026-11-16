import { memo, useEffect, useState } from 'react';
import { LoaderCircle } from 'lucide-react';
import type { RunStatus } from '../hooks/useAgentLiveSession';

const labels = {
  preparing_attachments: 'Preparing attachments',
  waiting_provider: 'Waiting for provider',
  receiving_answer: 'Receiving answer',
  running_tool: 'Running tool',
  awaiting_approval: 'Awaiting approval',
  saving_results: 'Saving results',
  completed: 'Answer complete',
  cancelled: 'Cancelled',
  error: 'Run error',
};

export interface ContextUsageInfo {
  usedTokens: number;
  maxTokens: number;
}

/** One clock in the visible status surface; no timers in message rows or tokens. */
export const RunStatusBar = memo(function RunStatusBar({
  status,
  onCancelPreparation,
  contextUsage,
  onCompact,
}: {
  status?: RunStatus | null;
  onCancelPreparation?: () => void;
  contextUsage?: ContextUsageInfo | null;
  onCompact?: () => void | Promise<void>;
}) {
  const [now, setNow] = useState(Date.now);
  const active = Boolean(status && !status.endedAt);
  useEffect(() => {
    if (!active) return;
    let timer: ReturnType<typeof setInterval> | undefined;
    const sync = () => {
      if (timer !== undefined) clearInterval(timer);
      timer = undefined;
      if (!document.hidden) {
        setNow(Date.now());
        timer = setInterval(() => setNow(Date.now()), 1000);
      }
    };
    sync();
    document.addEventListener('visibilitychange', sync);
    return () => {
      if (timer !== undefined) clearInterval(timer);
      document.removeEventListener('visibilitychange', sync);
    };
  }, [active, status?.runId, status?.sessionId]);

  const hasContextMeter = Boolean(contextUsage && contextUsage.usedTokens > 0 && contextUsage.maxTokens > 0);
  if (!status && !hasContextMeter) return null;

  const time = status?.endedAt ?? now;
  const seconds = (since: number) => Math.max(0, Math.floor((time - since) / 1000));
  const outcome = status?.outcome;
  let label: string = status ? labels[status.phase] : '';
  if (status?.saveFailed) {
    label = `${outcome?.answer === 'completed' ? 'Answer complete' : outcome?.answer === 'cancelled' ? 'Cancelled' : 'Run error'} · Save failed`;
  } else if (status?.endedAt && outcome) {
    const changes = outcome.filesChanged === true ? 'Changes applied' : outcome.filesChanged === false ? 'No file changes recorded' : 'File changes unknown';
    const verification = outcome.verification === 'passed' ? 'Verification passed' : outcome.verification === 'failed' ? 'Verification failed' : 'Verification not recorded';
    label += ` · ${changes} · ${verification}`;
  }

  return (
    <div
      className="mx-auto flex max-w-3xl flex-wrap items-center justify-between gap-x-3 gap-y-1 px-1 py-1.5 text-xs text-[#8b949e]"
      data-run-id={status?.runId ?? undefined}
    >
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        {status && (
          <>
            <span role="status" aria-live="polite" className="flex items-center gap-2">
              {active && <LoaderCircle size={13} aria-hidden="true" className="motion-safe:animate-spin" />}
              {label}
            </span>
            {status.phase === 'preparing_attachments' && onCancelPreparation && (
              <button
                type="button"
                onClick={onCancelPreparation}
                className="rounded px-2 py-1 text-[#e6edf3] hover:bg-[#30363d] focus-visible:outline focus-visible:outline-2"
              >
                Cancel preparation
              </button>
            )}
            <span aria-live="off">{seconds(status.startedAt)}s elapsed · {seconds(status.phaseSince)}s in stage</span>
          </>
        )}
      </div>

      {hasContextMeter && contextUsage && (() => {
        const { usedTokens, maxTokens } = contextUsage;
        const rawPct = Math.round((usedTokens / maxTokens) * 100);
        const barPct = Math.min(100, rawPct);
        const formatK = (n: number) => {
          if (n >= 1000000) return n % 1000000 === 0 ? `${n / 1000000}m` : `${(n / 1000000).toFixed(1)}m`;
          if (n >= 1000) return n % 1000 === 0 ? `${n / 1000}k` : `${(n / 1000).toFixed(1)}k`;
          return `${n}`;
        };
        const color = rawPct >= 80 ? 'text-[#f85149] font-medium' : rawPct >= 60 ? 'text-[#d29922]' : 'text-[#8b949e]';
        const barColor = rawPct >= 80 ? 'bg-[#f85149]' : rawPct >= 60 ? 'bg-[#d29922]' : 'bg-[#58a6ff]';
        const meterText = `${formatK(usedTokens)} / ${formatK(maxTokens)} tokens (${rawPct}%)`;

        return (
          <div
            className="ml-auto flex items-center gap-1.5 text-[11px] font-mono text-[#8b949e]"
            title={`Context usage: ${usedTokens.toLocaleString()} of ${maxTokens.toLocaleString()} tokens (${rawPct}%)${rawPct >= 80 ? ' - High context usage, consider /compact or starting a new chat' : ''}`}
            data-testid="context-meter"
          >
            <span className="w-12 h-1.5 rounded-full bg-[#21262d] overflow-hidden inline-flex border border-[#30363d]/50">
              <span className={`h-full ${barColor}`} style={{ width: `${barPct}%` }} />
            </span>
            <span className={color}>{meterText}</span>
            {rawPct >= 80 && onCompact && (
              <button
                type="button"
                onClick={onCompact}
                className="ml-0.5 text-[10px] px-1.5 py-0.5 rounded border border-[#f85149]/40 bg-[#f85149]/10 text-[#f85149] hover:bg-[#f85149]/20 transition-colors"
                title="Compact session context"
              >
                Compact
              </button>
            )}
          </div>
        );
      })()}
    </div>
  );
});
