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

/** One clock in the visible status surface; no timers in message rows or tokens. */
export const RunStatusBar = memo(function RunStatusBar({ status, onCancelPreparation }: { status?: RunStatus | null; onCancelPreparation?: () => void }) {
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
  if (!status) return null;
  const time = status.endedAt ?? now;
  const seconds = (since: number) => Math.max(0, Math.floor((time - since) / 1000));
  const outcome = status.outcome;
  let label: string = labels[status.phase];
  if (status.saveFailed) label = `${outcome?.answer === 'completed' ? 'Answer complete' : outcome?.answer === 'cancelled' ? 'Cancelled' : 'Run error'} · Save failed`;
  else if (status.endedAt && outcome) {
    const changes = outcome.filesChanged === true ? 'Changes applied' : outcome.filesChanged === false ? 'No file changes recorded' : 'File changes unknown';
    const verification = outcome.verification === 'passed' ? 'Verification passed' : outcome.verification === 'failed' ? 'Verification failed' : 'Verification not recorded';
    label += ` · ${changes} · ${verification}`;
  }
  return (
    <div className="mx-auto flex max-w-3xl flex-wrap items-center gap-x-3 gap-y-1 px-1 py-2 text-xs text-[#8b949e]" data-run-id={status.runId ?? undefined}>
      <span role="status" aria-live="polite" className="flex items-center gap-2">
        {active && <LoaderCircle size={13} aria-hidden="true" className="motion-safe:animate-spin" />}
        {label}
      </span>
      {status.phase === 'preparing_attachments' && onCancelPreparation && <button type="button" onClick={onCancelPreparation} className="rounded px-2 py-1 text-[#e6edf3] hover:bg-[#30363d] focus-visible:outline focus-visible:outline-2">Cancel preparation</button>}
      <span aria-live="off">{seconds(status.startedAt)}s elapsed · {seconds(status.phaseSince)}s in stage</span>
    </div>
  );
});
