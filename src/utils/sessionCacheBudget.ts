import type { LiveSession } from '../hooks/useAgentLiveSession';

export const SESSION_CACHE_BUDGET = 32 * 1024 * 1024;
const snapshotBytes = new WeakMap<object, number>();
/** Approximate retained UTF-16 strings, keys and object/array storage. No JSON copies.
 * Call only for immutable, clean snapshots; never from the streaming frame path.
 * Shared objects within a snapshot pair are counted once; cross-entry sharing may
 * be overestimated, deliberately preferring eviction over retained image payloads.
 */
export function estimateRetainedBytes(...roots: unknown[]): number {
  const unique = [...new Set(roots.filter((root): root is object => root !== null && typeof root === 'object'))];
  if (unique.length === 1) {
    const cached = snapshotBytes.get(unique[0]);
    if (cached !== undefined) return cached;
  }
  const seen = new WeakSet<object>();
  const stack: unknown[] = [...unique];
  let bytes = 0;
  while (stack.length) {
    const value = stack.pop();
    if (typeof value === 'string') { bytes += value.length * 2; continue; }
    if (typeof value !== 'object' || value === null) { bytes += 8; continue; }
    if (seen.has(value)) continue;
    seen.add(value);
    bytes += 32;
    if (value instanceof ArrayBuffer) { bytes += value.byteLength; continue; }
    if (ArrayBuffer.isView(value)) { stack.push(value.buffer); continue; }
    for (const key of Object.keys(value)) {
      bytes += key.length * 2 + 8;
      stack.push((value as Record<string, unknown>)[key]);
    }
  }
  if (unique.length === 1) snapshotBytes.set(unique[0], bytes);
  return bytes;
}
const pairBytes = new WeakMap<object, { committed: unknown; bytes: number }>();
function retained(live: LiveSession): number {
  const cached = pairBytes.get(live.data);
  if (cached && cached.committed === live.committedData) return cached.bytes;
  const bytes = estimateRetainedBytes(live.data, live.committedData);
  pairBytes.set(live.data, { committed: live.committedData, bytes });
  return bytes;
}
export function pruneSessionCache(cache: Map<string, LiveSession>, current: string, budget = SESSION_CACHE_BUDGET, maxInactive = 6): string[] {
  const candidates: [string, LiveSession, number][] = [];
  let total = 0;
  let inactive = 0;
  for (const [id, live] of cache) {
    if (id !== current) inactive++;
    const protectedEntry = live.runId || live.dirty || live.persistenceBusy || live.checkpointBusy || live.deltaBusy || live.loadingOlder || live.starting;
    // Protected streaming/dirty entries may exceed the cap. Avoid measuring a
    // changing graph, but include clean current snapshots in the byte budget.
    if (protectedEntry) continue;
    const bytes = retained(live);
    total += bytes;
    if (id !== current) candidates.push([id, live, bytes]);
  }
  const evicted: string[] = [];
  for (const [id, live, bytes] of candidates) {
    if (inactive <= maxInactive && total <= budget) break;
    if (live.checkpointTimer !== null) clearTimeout(live.checkpointTimer);
    cache.delete(id);
    total -= bytes;
    inactive--;
    evicted.push(id);
  }
  return evicted;
}

export type SessionLoadMetric = { kind: 'cold' | 'cache' | 'older'; durationMs: number; loaded: number; total: number; at: number };
const performanceSamples: SessionLoadMetric[] = [];
/** Local bounded diagnostics only: no session IDs, titles, contents or telemetry. */
export function recordSessionLoad(metric: SessionLoadMetric): void {
  performanceSamples.push({ ...metric, durationMs: Math.max(0, metric.durationMs) });
  if (performanceSamples.length > 100) performanceSamples.shift();
}
export function getSessionPerformanceSnapshot(): SessionLoadMetric[] {
  return performanceSamples.map((sample) => ({ ...sample }));
}

/** Returns cancellation for both pending idle callbacks and subsequent batches.
 * A native batch already running can finish; it never publishes session UI.
 */
export function scheduleSessionIndexWarmup(isIdle: () => boolean, warm: () => Promise<{ remaining: number; errors: number }>): () => void {
  let cancelled = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let idle: number | undefined;
  const schedule = () => {
    if (cancelled) return;
    if (typeof window.requestIdleCallback === 'function') idle = window.requestIdleCallback(() => { void tick(); });
    else timer = setTimeout(() => { void tick(); }, 1000);
  };
  const tick = async () => {
    if (cancelled) return;
    if (!isIdle()) { timer = setTimeout(schedule, 1000); return; }
    try {
      const result = await warm();
      if (!cancelled && result && result.remaining > 0 && result.errors === 0) timer = setTimeout(schedule, 1000);
    } catch { /* optional maintenance must not obscure foreground errors */ }
  };
  schedule();
  return () => { cancelled = true; if (timer !== undefined) clearTimeout(timer); if (idle !== undefined && typeof window.cancelIdleCallback === 'function') window.cancelIdleCallback(idle); };
}
