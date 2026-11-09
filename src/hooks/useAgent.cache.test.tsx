import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSettings, SessionData } from '../types/agent';
import { useAgent } from './useAgent';
import { getSessionPerformanceSnapshot } from '../utils/sessionCacheBudget';

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, (event: { payload: unknown }) => void>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
  mocks.listeners.set(name, handler);
  return () => mocks.listeners.delete(name);
}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));
const provider = { id: 'p', name: 'Provider', kind: 'openai' as const, baseUrl: 'http://localhost/v1', apiKey: '', model: 'model' };
const settings: AgentSettings = {
  providers: [provider], activeProviderId: 'p', systemPrompt: '', temperature: 0.7, streamResponse: true,
  thinking: false, reasoningEffort: 'medium', favoriteModels: [], skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const initial: SessionData = {
  session: { id: 'one', title: 'Original', createdAt: 1, updatedAt: 2, messageCount: 102 },
  messages: [{ id: 'u100', role: 'user', content: 'Prompt', timestamp: 100 }, { id: 'a101', role: 'assistant', content: 'Answer', timestamp: 101 }],
  artifacts: [],
};
const window = { offset: 100, total: 102, revision: 'r1', firstMessageId: 'u100' };
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  return hook;
}
beforeEach(() => {
  mocks.listeners.clear();
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [initial.session];
    if (command === 'load_session_page') return { data: { ...structuredClone(initial), session: { ...initial.session, id: args.sessionId } }, window: { ...window } };
    if (command === 'save_session_page' || command === 'checkpoint_session_page') return { ...args.window, revision: 'r2', total: args.window.offset + args.data.messages.length };
    if (command === 'load_session') return structuredClone(initial);
    return undefined;
  });
});
afterEach(cleanup);


describe('useAgent bounded history cache', () => {
  it('replays unchanged cached messages with no IPC reads or writes and records loads', async () => {
    const hook = await mounted();
    const original = hook.result.current.messages;
    await act(async () => { await hook.result.current.switchSession('two'); });
    const calls = mocks.invoke.mock.calls.length;
    await act(async () => { await hook.result.current.switchSession('one'); });
    expect(hook.result.current.messages).toBe(original);
    expect(mocks.invoke.mock.calls.slice(calls).filter(([command]) => command === 'load_session_page' || command === 'save_session_page' || command === 'save_session')).toHaveLength(0);
    expect(getSessionPerformanceSnapshot().slice(-2).map((sample) => sample.kind)).toEqual(['cold','cache']);
  });
  it('evicts oversized inactive snapshots and reloads them on the next visit', async () => {
    const hook = await mounted();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
      const response = await backend(command, args);
      if (command === 'load_session_page' && args.sessionId === 'large') response.data.messages[0].content = 'x'.repeat(17*1024*1024);
      return response;
    });
    await act(async () => { await hook.result.current.switchSession('large'); });
    await act(async () => { await hook.result.current.switchSession('one'); });
    await act(async () => { await hook.result.current.switchSession('large'); });
    expect(mocks.invoke.mock.calls.filter(([command,args]) => command === 'load_session_page' && args.sessionId === 'large')).toHaveLength(2);
  });
  it('does not record cancelled navigation as a successful load', async () => {
    const hook = await mounted();
    const pending = deferred<{data:SessionData;window:typeof window}>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string,args:Record<string,any>) => command === 'load_session_page' && args.sessionId === 'two' ? pending.promise : backend(command,args));
    const samples = getSessionPerformanceSnapshot().length;
    let switching!: Promise<void>;
    act(() => { switching=hook.result.current.switchSession('two'); });
    await act(async () => { await hook.result.current.switchSession('one'); });
    await act(async () => { pending.resolve({data:initial,window}); await switching; });
    expect(getSessionPerformanceSnapshot()).toHaveLength(samples);
    expect(hook.result.current.currentSessionId).toBe('one');
  });
});

it('records initial bounded restore as a cold load', async () => {
  const before = getSessionPerformanceSnapshot().length;
  const hook = await mounted();
  const sample = getSessionPerformanceSnapshot().at(-1);
  expect(getSessionPerformanceSnapshot()).toHaveLength(before + 1);
  expect(sample).toEqual(expect.objectContaining({kind:'cold',loaded:2,total:102}));
  expect(hook.result.current.currentSessionId).toBe('one');
});
