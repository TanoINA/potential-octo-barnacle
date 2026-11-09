import { pagingBackend } from '../test/sessionPaging';
import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSettings, SessionData } from '../types/agent';
import { useAgent } from './useAgent';

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));

const provider = { id: 'provider', name: 'Provider', kind: 'openai' as const, baseUrl: 'http://localhost:1234/v1', apiKey: '', model: 'model' };
const settings: AgentSettings = {
  providers: [provider], activeProviderId: provider.id, systemPrompt: '', temperature: 0.7,
  streamResponse: true, thinking: false, reasoningEffort: 'medium', favoriteModels: [],
  skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const initial: SessionData = {
  session: { id: 'sess-1', title: 'Original', createdAt: 1, updatedAt: 2, messageCount: 2 },
  messages: [
    { id: 'user-1', role: 'user', content: 'Original prompt', timestamp: 1 },
    { id: 'assistant-1', role: 'assistant', content: 'Original answer', timestamp: 2 },
  ], artifacts: [],
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  expect(hook.result.current.currentSessionId).toBe('sess-1');
  return hook;
}
beforeEach(() => {
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(pagingBackend(async (command: string) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [initial.session];
    if (command === 'load_session') return structuredClone(initial);
    return undefined;
  }));
});
afterEach(cleanup);

describe('useAgent navigation', () => {
  it('allows sending after clear cancels a deferred session load', async () => {
    const hook = await mounted();
    const load = deferred<SessionData>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(pagingBackend((command: string, args: unknown) => command === 'load_session' ? load.promise : backend(command, args)));
    let switching!: Promise<void>;
    act(() => { switching = hook.result.current.switchSession('sess-2'); });
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('load_session_page', { sessionId: 'sess-2', limit: 50 }));
    act(() => hook.result.current.clearCurrentSession());
    const other = structuredClone(initial);
    other.session.id = 'sess-2';
    await act(async () => { load.resolve(other); await switching; });
    expect(hook.result.current.currentSessionId).toBe('sess-1');
    expect(hook.result.current.messages).toHaveLength(0);
    await act(async () => { expect(await hook.result.current.sendMessage('After clear')).toBe(true); });
    expect(mocks.invoke).toHaveBeenCalledWith('send_message', expect.objectContaining({ request: expect.objectContaining({ sessionId: 'sess-1' }) }));
  });
  it.each(['resolve', 'reject'] as const)('discards undo after new session when persistence will %s', async (outcome) => {
    const hook = await mounted();
    const save = deferred<void>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(pagingBackend((command: string, args: unknown) => command === 'save_session' ? save.promise : backend(command, args)));
    let undo!: ReturnType<typeof hook.result.current.undoTurn>;
    let retried!: Promise<void>;
    let undone: Awaited<typeof undo>;
    act(() => {
      undo = hook.result.current.undoTurn();
      // Mirror the slash /retry consumer: a truthy result resends into the viewed session.
      retried = undo.then(async (result) => {
        undone = result;
        if (result) await hook.result.current.sendMessage(result.content, result.attachments);
      });
    });
    await waitFor(() => expect(mocks.invoke.mock.calls.some(([name]) => name === 'save_session')).toBe(true));
    act(() => hook.result.current.newSession());
    const newId = hook.result.current.currentSessionId;
    await act(async () => {
      if (outcome === 'resolve') save.resolve();
      else save.reject(new Error('stale undo save failure'));
      await retried;
    });
    expect(undone!).toBeNull();
    expect(hook.result.current.currentSessionId).toBe(newId);
    expect(hook.result.current.messages).toHaveLength(0);
    expect(hook.result.current.lastError).toBeNull();
    expect(hook.result.current.isLoading).toBe(false);
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'send_message')).toHaveLength(0);
  });
  it('returns the original prompt for an undo that stays in its origin', async () => {
    const hook = await mounted();
    await act(async () => {
      expect(await hook.result.current.undoTurn()).toEqual({ content: 'Original prompt', attachments: undefined });
    });
    expect(hook.result.current.messages).toHaveLength(0);
    expect(hook.result.current.isLoading).toBe(false);
    expect(hook.result.current.lastError).toBeNull();
  });

  it('discards undo after clear changes generation without changing session identity', async () => {
    const hook = await mounted();
    const save = deferred<void>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(pagingBackend((command: string, args: unknown) => command === 'save_session' ? save.promise : backend(command, args)));
    let undo!: ReturnType<typeof hook.result.current.undoTurn>;
    act(() => { undo = hook.result.current.undoTurn(); });
    await waitFor(() => expect(mocks.invoke.mock.calls.some(([name]) => name === 'save_session')).toBe(true));
    act(() => hook.result.current.clearCurrentSession());
    await act(async () => { save.resolve(); expect(await undo).toBeNull(); });
    expect(hook.result.current.currentSessionId).toBe('sess-1');
    expect(hook.result.current.messages).toHaveLength(0);
    await act(async () => { expect(await hook.result.current.sendMessage('Fresh intent')).toBe(true); });
  });
});
