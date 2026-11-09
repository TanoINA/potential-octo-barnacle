import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useModalAccessibility } from '../hooks/useModalAccessibility';
import { getSessionPerformanceSnapshot } from '../utils/sessionCacheBudget';

type ReleaseInfo = { version: string; buildId: string; platform: string; arch: string; diagnosticLogsPath: string; signingStatus: string; updatesConfigured: boolean; updateStatus: string };
type InstallResult = { status: string; restartRequired: boolean };
type UpdateResult = { version?: string; status: string; notes?: string };
type UpdateProgress = { phase: 'downloading' | 'verifying' | 'installing'; downloaded: number; total?: number };
const bounded = (text: string, limit = 1000) => text.length > limit ? `${text.slice(0, limit)}…` : text;
export function AboutModal({ onClose, onBeforeInstall }: { onClose: () => void; onBeforeInstall?: () => Promise<void> }) {
  const [info, setInfo] = useState<ReleaseInfo | null>(null);
  const [error, setError] = useState('');
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [update, setUpdate] = useState<UpdateResult | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [installing, setInstalling] = useState(false);
  const nativeOperation = useRef(false);
  const [restartRequired, setRestartRequired] = useState(false);
  const [metrics] = useState(getSessionPerformanceSnapshot);
  const active = useRef(true);
  const pending = useRef(false);
  const stopProgress = useRef<(() => void) | null>(null);
  const close = () => {
    if (nativeOperation.current) return;
    active.current = false;
    stopProgress.current?.();
    onClose();
  };
  const ref = useModalAccessibility<HTMLDivElement>(true, close);
  useEffect(() => {
    active.current = true;
    return () => {
      active.current = false;
      stopProgress.current?.();
    };
  }, []);
  useEffect(() => {
    let current = true;
    setError('');
    const load = async () => {
      try {
        const value = await invoke<ReleaseInfo>('get_release_info');
        if (current && active.current) setInfo(value);
      } catch (reason) {
        if (current && active.current) setError(String(reason));
      }
    };
    void load();
    return () => { current = false; };
  }, [loadAttempt]);
  const check = async () => {
    if (!active.current || pending.current || !info?.updatesConfigured) return;
    pending.current = true;
    setBusy(true);
    setError('');
    // A new native check invalidates the previously offered installer.
    setUpdate(null);
    try {
      const result = await invoke<UpdateResult>('check_release_update');
      if (active.current) setUpdate(result);
    } catch (reason) {
      if (active.current) setError(String(reason));
    } finally {
      pending.current = false;
      if (active.current) setBusy(false);
    }
  };
  const install = async () => {
    if (!active.current || pending.current || !update?.version || !window.confirm(`Install signed update ${update.version}? AetherAgent may close. Save your work first.`)) return;
    pending.current = true;
    setBusy(true);
    setError('');
    setProgress(null);
    let receiving = false;
    let finished = false;
    let unlisten: (() => void) | undefined;
    const stop = () => {
      finished = true;
      receiving = false;
      const release = unlisten;
      unlisten = undefined;
      release?.();
      if (stopProgress.current === stop) stopProgress.current = null;
    };
    stopProgress.current = stop;
    try {
      if (onBeforeInstall) await onBeforeInstall();
      if (!active.current || finished) return;
      unlisten = await listen<UpdateProgress>('release:update-progress', ({ payload }) => {
        if (!active.current || !receiving || finished) return;
        if (!payload || !['downloading', 'verifying', 'installing'].includes(payload.phase)) return;
        setProgress(payload);
      });
      if (!active.current || finished) return;
      receiving = true;
      nativeOperation.current = true;
      setInstalling(true);
      const result = await invoke<InstallResult | void>('install_release_update', { expectedVersion: update.version, confirmed: true });
      if (active.current) {
        setUpdate({ status: result?.status ?? 'Installer started; complete the installer to finish updating.' });
        setRestartRequired(result?.restartRequired === true);
      }
    } catch (reason) {
      if (active.current) setError(String(reason));
    } finally {
      stop();
      nativeOperation.current = false;
      if (active.current) setInstalling(false);
      pending.current = false;
      if (active.current) {
        setProgress(null);
        setBusy(false);
      }
    }
  };
  const restart = async () => {
    if (!active.current || pending.current || !restartRequired || !window.confirm('Restart AetherAgent to finish updating? All sessions must be idle and saved.')) return;
    pending.current = true;
    setBusy(true);
    setError('');
    try {
      if (onBeforeInstall) await onBeforeInstall();
      if (!active.current) return;
      await invoke('restart_after_update', { confirmed: true });
    } catch (reason) {
      if (active.current) setError(String(reason));
    } finally {
      pending.current = false;
      if (active.current) setBusy(false);
    }
  };
  const percentage = progress?.phase === 'downloading' && Number.isFinite(progress.downloaded) && Number.isFinite(progress.total) && progress.total! > 0
    ? Math.min(100, Math.max(0, progress.downloaded / progress.total! * 100))
    : undefined;
  const progressStatus = progress && { downloading: 'Downloading signed update…', verifying: 'Verifying signed update…', installing: 'Installing signed update…' }[progress.phase];
  return <div className="ui-overlay-enter fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"><div ref={ref} role="dialog" aria-modal="true" aria-labelledby="about-title" tabIndex={-1} className="ui-panel-enter max-h-[92vh] overflow-y-auto w-full max-w-xl rounded-xl border border-[#30363d] bg-[#161b22] p-6 space-y-4">
    <header className="flex justify-between"><h2 id="about-title">About AetherAgent</h2><button type="button" onClick={close} disabled={installing} aria-label="Close About">Close</button></header>
    {!info && !error && <p>Loading release information...</p>}
    {info && <><dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm break-all"><dt>Version</dt><dd>{info.version}</dd><dt>Build</dt><dd>{info.buildId}</dd><dt>Platform</dt><dd>{info.platform} / {info.arch}</dd><dt>Installer signing</dt><dd>{info.signingStatus}</dd><dt>Diagnostic task logs</dt><dd>{info.diagnosticLogsPath}</dd></dl>
      <button type="button" className="toolbar-button" onClick={() => { void invoke('open_path_or_url', { pathOrUrl: info.diagnosticLogsPath }).catch((reason) => { if (active.current) setError(String(reason)); }); }}>Open diagnostic logs</button>
      <p>{bounded(info.updateStatus)}</p><button type="button" className="toolbar-button" disabled={!info.updatesConfigured || busy} onClick={() => { void check(); }}>Check for updates</button>
      {update && <p role="status">{progressStatus ?? bounded(update.status)}</p>}
      {progress && <progress aria-label="Update download progress" max={100} value={percentage} />}
      {installing && <p className="text-xs text-[#8b949e]">Keep this window open while the signed update is being installed.</p>}
      {update?.notes && <section role="region" aria-label="Release notes" className="max-h-48 overflow-y-auto whitespace-pre-wrap break-words text-sm">{bounded(update.notes, 12000)}</section>}
      {update?.version && <button type="button" disabled={busy} className="toolbar-button" onClick={() => { void install(); }}>Install signed update {update.version}</button>}
      {restartRequired && <><p>Restart required to finish updating. AetherAgent will not restart automatically.</p><button type="button" disabled={busy} className="toolbar-button" onClick={() => { void restart(); }}>Restart to finish update</button></>}
      <p className="text-xs text-[#8b949e]">Manual installation and rollback require a separately selected installer, independently obtained SHA-256 and trusted publisher certificate. No automatic downgrade.</p>
    </>}
    <section><h3>Local session-load diagnostics</h3><p className="text-xs">{metrics.length ? `${metrics.length} recent loads; mean ${(metrics.reduce((sum, metric) => sum + metric.durationMs, 0) / metrics.length).toFixed(1)} ms` : 'No local load samples yet.'} No telemetry or conversation content.</p></section>
    {error && <p role="alert">{error}</p>}{!info && error && <button type="button" onClick={() => setLoadAttempt((attempt) => attempt + 1)}>Retry</button>}
  </div></div>;
}


