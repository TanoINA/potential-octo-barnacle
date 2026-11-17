import { pagingBackend } from '../test/sessionPaging';
import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { BackgroundTask, SessionData } from '../types/agent';
import { useAgent } from './useAgent';

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));

function session(id: string): SessionData {
  return { session: { id, title: id, createdAt: 1, updatedAt: 1, messageCount: 0 }, messages: [], artifacts: [] };
}
function task(sessionId: string, id = 'task-1'): BackgroundTask & { sessionId: string } {
  return { id, sessionId, command: 'test', isDaemon: false, status: 'completed', startedAt: 1, logPath: '', recentOutput: '' };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const taskCalls = () => mocks.invoke.mock.calls.filter(([name]) => name === 'list_background_tasks');

async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  expect(hook.result.current.currentSessionId).toBe('session-a');
  return hook;
}

beforeEach(() => {
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(pagingBackend(async (name: string, args?: { sessionId?: string }) => {
    if (name === 'get_settings') return { providers: [], activeProviderId: '' };
    if (name === 'list_sessions') return [session('session-a').session];
    if (name === 'load_session') return session(args!.sessionId!);
    if (name === 'list_background_tasks') return [task(args?.sessionId ?? 'unscoped')];
    return undefined;
  }));
});

describe('useAgent session-owned background tasks', () => {
  it('continues polling stopping daemons until the worker reports a terminal state', async () => {
    const hook = await mounted();
    const backend = mocks.invoke.getMockImplementation()!;
    let status: BackgroundTask['status'] = 'stopping';
    mocks.invoke.mockImplementation(pagingBackend((name: string, args?: { sessionId?: string }) =>
      name === 'list_background_tasks' ? [{ ...task('session-a'), isDaemon: true, status }] : backend(name, args)));
    vi.useFakeTimers();
    try {
      await act(async () => hook.result.current.refreshBackgroundTasks());
      const before = taskCalls().length;
      status = 'cancelled';
      await act(async () => vi.advanceTimersByTimeAsync(2000));
      expect(taskCalls().length).toBe(before + 1);
      expect(hook.result.current.backgroundTasks[0].status).toBe('cancelled');
      await act(async () => vi.advanceTimersByTimeAsync(4000));
      expect(taskCalls().length).toBe(before + 1);
    } finally {
      hook.unmount();
      vi.useRealTimers();
    }
  });

  it('scopes killing to the origin and ignores errors arriving after navigation', async () => {
    const hook = await mounted();
    const killing = deferred<void>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(pagingBackend((name: string, args?: { sessionId?: string }) =>
      name === 'kill_background_task' ? killing.promise : backend(name, args)));
    let pending!: Promise<void>;
    act(() => { pending = hook.result.current.killBackgroundTask('task-1'); });
    expect(mocks.invoke).toHaveBeenCalledWith('kill_background_task', { sessionId: 'session-a', taskId: 'task-1' });
    act(() => hook.result.current.newSession());
    await act(async () => { killing.reject(new Error('old session failure')); await pending; });
    expect(hook.result.current.lastError).toBeNull();
  });

  it('does not publish a delayed task list into a different session', async () => {
    const hook = await mounted();
    const listing = deferred<BackgroundTask[]>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(pagingBackend((name: string, args?: { sessionId?: string }) =>
      name === 'list_background_tasks' && args?.sessionId === 'session-a' ? listing.promise : backend(name, args)));
    let pending!: Promise<void>;
    act(() => { pending = hook.result.current.refreshBackgroundTasks(); });
    act(() => hook.result.current.newSession());
    const next = hook.result.current.currentSessionId;
    await act(async () => { listing.resolve([task('session-a', 'private')]); await pending; });
    expect(hook.result.current.backgroundTasks.every((item) => item.sessionId === next)).toBe(true);
    expect(hook.result.current.backgroundTasks.some((item) => item.id === 'private')).toBe(false);
  });
  it('waits for startup restoration, then lists only the restored session', async () => {
    const loading = deferred<SessionData>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(pagingBackend((name: string, args?: { sessionId?: string }) =>
      name === 'load_session' ? loading.promise : backend(name, args)));
    const hook = renderHook(() => useAgent());
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('load_session_page', { sessionId: 'session-a', limit: 50 }));
    expect(taskCalls()).toEqual([]);
    await act(async () => loading.resolve(session('session-a')));
    await waitFor(() => expect(hook.result.current.backgroundTasks).toEqual([task('session-a')]));
    expect(taskCalls()).toEqual([['list_background_tasks', { sessionId: 'session-a' }]]);
  });
});
