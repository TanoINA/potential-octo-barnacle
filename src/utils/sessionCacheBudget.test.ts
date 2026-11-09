import { afterEach, describe, expect, it, vi } from 'vitest';
import { pruneSessionCache, estimateRetainedBytes, getSessionPerformanceSnapshot, recordSessionLoad, scheduleSessionIndexWarmup } from './sessionCacheBudget';
import { liveSession } from '../hooks/useAgentLiveSession';
import type { SessionData } from '../types/agent';
function entry(id: string, content = '') {
  return liveSession({ session: { id, title: id, createdAt: 1, updatedAt: 1, messageCount: 1 }, messages: [{ id, role: 'user', content, timestamp: 1 }], artifacts: [] } satisfies SessionData);
}
describe('session cache byte budget', () => {
  it('evicts a large inactive image payload even below the count cap', () => {
    const big = entry('image', 'x'.repeat(17 * 1024 * 1024));
    const cache = new Map([['image', big], ['current', entry('current')]]);
    expect(pruneSessionCache(cache, 'current')).toEqual(['image']);
    expect(cache.has('current')).toBe(true);
    expect(estimateRetainedBytes(big.data, big.data)).toBe(estimateRetainedBytes(big.data));
  });
});

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
it('retains protected entries and evicts oldest clean entries', () => {
  const cache = new Map(Array.from({length: 10}, (_, n) => [String(n), entry(String(n))]));
  cache.get('0')!.dirty = true;
  cache.get('1')!.runId = 'active';
  cache.get('2')!.persistenceBusy = 1;
  cache.get('3')!.deltaBusy = true;
  expect(pruneSessionCache(cache, '9', 0)).toEqual(['4', '5', '6', '7', '8']);
  expect([...cache.keys()]).toEqual(['0', '1', '2', '3', '9']);
});
it('enforces six inactive entries in visit order without changing snapshots', () => {
  const cache = new Map(Array.from({length: 9}, (_, n) => [String(n), entry(String(n))]));
  const data = cache.get('8')!.data;
  expect(pruneSessionCache(cache, '8')).toEqual(['0', '1']);
  expect(pruneSessionCache(cache, '8')).toEqual([]);
  expect(cache.get('8')!.data).toBe(data);
});
it('counts distinct committed snapshots but not shared message object graphs', () => {
  const live = entry('one', 'a'.repeat(100));
  const single = estimateRetainedBytes(live.data);
  const copy = { ...live.data };
  expect(estimateRetainedBytes(live.data, copy)).toBeLessThan(single * 2);
  live.committedData = entry('old', 'b'.repeat(500)).data;
  expect(estimateRetainedBytes(live.data, live.committedData)).toBeGreaterThan(single);
});
it('bounds diagnostic samples and returns defensive copies without content', () => {
  for (let n = 0; n < 105; n++) recordSessionLoad({kind:'cache',durationMs:n,loaded:n,total:1000,at:n});
  const snapshot = getSessionPerformanceSnapshot();
  expect(snapshot).toHaveLength(100);
  expect(snapshot[0].at).toBe(5);
  snapshot[0].loaded = -1;
  expect(getSessionPerformanceSnapshot()[0].loaded).toBe(5);
  expect(Object.keys(snapshot[0]).sort()).toEqual(['at','durationMs','kind','loaded','total']);
});
it('defers warmup while foreground is busy and cancels queued batches', async () => {
  vi.useFakeTimers();
  let idle = false;
  const warm = vi.fn(async () => ({remaining:2,errors:0}));
  const cancel = scheduleSessionIndexWarmup(() => idle, warm);
  await vi.advanceTimersByTimeAsync(1000);
  expect(warm).not.toHaveBeenCalled();
  idle = true;
  await vi.advanceTimersByTimeAsync(2000);
  expect(warm).toHaveBeenCalledTimes(1);
  cancel();
  await vi.advanceTimersByTimeAsync(10000);
  expect(warm).toHaveBeenCalledTimes(1);
});
it('does not schedule another batch when cancelled during a native call', async () => {
  vi.useFakeTimers();
  let resolve!: (value: {remaining:number;errors:number}) => void;
  const warm = vi.fn(() => new Promise<{remaining:number;errors:number}>((yes) => {resolve=yes;}));
  const cancel = scheduleSessionIndexWarmup(() => true, warm);
  await vi.advanceTimersByTimeAsync(1000);
  expect(warm).toHaveBeenCalledTimes(1);
  cancel();
  resolve({remaining:4,errors:0});
  await vi.advanceTimersByTimeAsync(10000);
  expect(warm).toHaveBeenCalledTimes(1);
});
