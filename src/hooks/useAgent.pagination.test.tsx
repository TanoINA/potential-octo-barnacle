import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSettings, SessionData } from '../types/agent';
import { useAgent } from './useAgent';

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

describe('useAgent paged history', () => {
  it.each(['resolve', 'reject'] as const)('cancels a deferred switch by reselecting the visible session before its load will %s', async (outcome) => {
    const hook = await mounted();
    expect(hook.result.current.currentSessionId).toBe('one');
    const messages = hook.result.current.messages;
    const pending = deferred<{ data: SessionData; window: typeof window }>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) =>
      command === 'load_session_page' && args.sessionId === 'two' ? pending.promise : backend(command, args));
    let switching!: Promise<void>;
    act(() => { switching = hook.result.current.switchSession('two'); });
    expect(hook.result.current.isSessionLoading).toBe(true);
    expect(hook.result.current.currentSessionId).toBe('one');
    await act(async () => { expect(await hook.result.current.sendMessage('Blocked')).toBe(false); });
    expect(hook.result.current.lastError).toMatch(/loading/i);
    await act(async () => { await hook.result.current.switchSession('one'); });
    // Assert before settling B: its stale finally must not be needed to unblock A.
    expect(hook.result.current.isSessionLoading).toBe(false);
    expect(hook.result.current.lastError).toBeNull();
    expect(hook.result.current.messages).toBe(messages);
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'save_session' || name === 'save_session_page')).toHaveLength(0);
    await act(async () => {
      if (outcome === 'resolve') pending.resolve({ data: { ...initial, session: { ...initial.session, id: 'two', title: 'Late B' }, messages: [] }, window: { ...window, offset: 0, total: 0 } });
      else pending.reject(new Error('Late B failure'));
      await switching;
    });
    expect(hook.result.current.currentSessionId).toBe('one');
    expect(hook.result.current.sessionTitle).toBe('Original');
    expect(hook.result.current.messages).toBe(messages);
    expect(hook.result.current.isSessionLoading).toBe(false);
    expect(hook.result.current.lastError).toBeNull();
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'save_session' || name === 'save_session_page')).toHaveLength(0);
    await act(async () => { expect(await hook.result.current.sendMessage('After cancellation')).toBe(true); });
    expect(mocks.invoke).toHaveBeenCalledWith('send_message', expect.objectContaining({ request: expect.objectContaining({ sessionId: 'one' }) }));
    const saves = mocks.invoke.mock.calls.filter(([name]) => name === 'save_session' || name === 'save_session_page');
    expect(saves.length).toBeGreaterThan(0);
    for (const [, args] of saves) {
      expect(args.data.session.id).toBe('one');
      expect(args.data.messages.slice(0, 2).map((message: { id: string }) => message.id)).toEqual(['u100', 'a101']);
    }
  });
  it.each(['before', 'after'] as const)('lets only the latest rapid switch own loading when B settles %s C', async (order) => {
    const hook = await mounted();
    const b = deferred<{ data: SessionData; window: typeof window }>();
    const c = deferred<{ data: SessionData; window: typeof window }>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => {
      if (command === 'load_session_page' && args.sessionId === 'two') return b.promise;
      if (command === 'load_session_page' && args.sessionId === 'three') return c.promise;
      return backend(command, args);
    });
    let switchingB!: Promise<void>;
    let switchingC!: Promise<void>;
    act(() => { switchingB = hook.result.current.switchSession('two'); });
    act(() => { switchingC = hook.result.current.switchSession('three'); });
    expect(hook.result.current.isSessionLoading).toBe(true);
    if (order === 'before') {
      await act(async () => {
        b.resolve({ data: { ...initial, session: { ...initial.session, id: 'two' } }, window: { ...window } });
        await switchingB;
      });
      expect(hook.result.current.isSessionLoading).toBe(true);
      expect(hook.result.current.currentSessionId).toBe('one');
    }
    await act(async () => {
      c.resolve({ data: { ...initial, session: { ...initial.session, id: 'three', title: 'Winner C' } }, window: { ...window } });
      await switchingC;
    });
    expect(hook.result.current.isSessionLoading).toBe(false);
    expect(hook.result.current.currentSessionId).toBe('three');
    if (order === 'after') {
      await act(async () => { b.reject(new Error('Superseded B failure')); await switchingB; });
    }
    expect(hook.result.current.currentSessionId).toBe('three');
    expect(hook.result.current.sessionTitle).toBe('Winner C');
    expect(hook.result.current.isSessionLoading).toBe(false);
    expect(hook.result.current.lastError).toBeNull();
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'save_session' || name === 'save_session_page')).toHaveLength(0);
  });
  it('saves a visible tail with its cursor and sends without full hydration', async () => {
    const hook = await mounted();
    await act(async () => { expect(await hook.result.current.sendMessage('Next')).toBe(true); });
    expect(mocks.invoke).toHaveBeenCalledWith('save_session_page', expect.objectContaining({ window: expect.objectContaining({ offset: 100, revision: 'r1' }) }));
    expect(mocks.invoke).toHaveBeenCalledWith('send_message', expect.objectContaining({ request: expect.objectContaining({ historyWindow: expect.objectContaining({ offset: 100, revision: 'r2' }) }) }));
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'load_session')).toBe(false);
  });
  it('reuses clean cached pages without saving or rereading', async () => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.switchSession('two'); });
    await act(async () => { await hook.result.current.switchSession('one'); });
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'load_session_page')).toHaveLength(2);
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'save_session' || name === 'save_session_page')).toHaveLength(0);
    expect(hook.result.current.currentSessionId).toBe('one');
  });
  it('bounds clean inactive cache entries to six', async () => {
    const hook = await mounted();
    for (let i = 0; i < 8; i++) await act(async () => { await hook.result.current.switchSession('cache-' + i); });
    const count = mocks.invoke.mock.calls.filter(([name]) => name === 'load_session_page').length;
    await act(async () => { await hook.result.current.switchSession('one'); });
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'load_session_page')).toHaveLength(count + 1);
  });
  it('prepends older history while retaining a tail added during the read', async () => {
    const hook = await mounted();
    const pending = deferred<{ data: SessionData; window: typeof window }>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => command === 'load_session_page' && args.before !== undefined ? pending.promise : backend(command, args));
    let loading!: Promise<void>;
    act(() => { loading = hook.result.current.loadOlderMessages(); });
    await waitFor(() => expect(hook.result.current.loadingOlderMessages).toBe(true));
    await act(async () => { await hook.result.current.sendMessage('New streamed tail'); });
    const scope = { sessionId: 'one', runId: hook.result.current.messages.at(-1)!.runId! };
    act(() => { mocks.listeners.get('agent-event')!({ payload: { type: 'chunk', data: { ...scope, text: 'Kept text' } } }); });
    await act(async () => {
      pending.resolve({ data: { ...initial, messages: [{ id: 'older', role: 'user', content: 'Old', timestamp: 1 }] }, window: { ...window, offset: 99, firstMessageId: 'older' } });
      await loading;
    });
    await waitFor(() => expect(hook.result.current.messages.at(-1)!.content).toBe('Kept text'));
    expect(hook.result.current.messages.map((m) => m.id).slice(0, 3)).toEqual(['older', 'u100', 'a101']);
    expect(hook.result.current.totalMessageCount).toBe(104);
    expect(hook.result.current.hasOlderMessages).toBe(true);
  });
  it('reloads the canonical tail on explicit retry instead of trusting one boundary', async () => {
    const hook = await mounted();
    const backend = mocks.invoke.getMockImplementation()!;
    let stale = true;
    const messages = [
      ...Array.from({ length: 100 }, (_, i) => ({ id: 'old' + i, role: 'user' as const, content: 'Old', timestamp: i })),
      ...initial.messages.map((message) => ({ ...message, content: 'External edit' })),
    ];
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => {
      if (command !== 'load_session_page') return backend(command, args);
      if (stale) { stale = false; return Promise.reject(new Error('stale cursor revision')); }
      const end = args.before ?? messages.length;
      const offset = Math.max(0, end - args.limit);
      return Promise.resolve({ data: { ...initial, messages: messages.slice(offset, end) }, window: {
        offset, total: messages.length, revision: 'external', firstMessageId: messages[offset]?.id ?? null,
      } });
    });
    await act(async () => { await hook.result.current.loadOlderMessages(); });
    expect(hook.result.current.historyError).toMatch(/retry/i);
    expect(hook.result.current.messages).toHaveLength(2);
    await act(async () => { await hook.result.current.loadOlderMessages(); });
    expect(mocks.invoke).toHaveBeenCalledWith('load_session_page', { sessionId: 'one', limit: 50 });
    expect(hook.result.current.historyError).toBeNull();
    expect(hook.result.current.messages).toHaveLength(52);
    expect(hook.result.current.messages.at(-1)?.content).toBe('External edit');
  });
  it('ignores an older page that resolves after navigation', async () => {
    const hook = await mounted();
    const pending = deferred<any>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => command === 'load_session_page' && args.before !== undefined ? pending.promise : backend(command, args));
    let loading!: Promise<void>;
    act(() => { loading = hook.result.current.loadOlderMessages(); });
    await act(async () => { await hook.result.current.switchSession('two'); });
    await act(async () => { pending.resolve({ data: { ...initial, messages: [] }, window: { ...window, offset: 0 } }); await loading; });
    expect(hook.result.current.currentSessionId).toBe('two');
    expect(hook.result.current.messages).toHaveLength(2);
    expect(hook.result.current.loadingOlderMessages).toBe(false);
  });
  it('clears the complete session with a full replacement, never a page merge', async () => {
    const hook = await mounted();
    act(() => { hook.result.current.clearCurrentSession(); });
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('save_session', { data: expect.objectContaining({ messages: [], artifacts: [] }) }));
    const save = mocks.invoke.mock.calls.find(([name]) => name === 'save_session')![1];
    expect(save.data.historyWindow).toBeUndefined();
    expect(hook.result.current.hasOlderMessages).toBe(false);
  });
  it.each(['undo', 'rollback', 'edit'] as const)('hydrates hidden history and artifacts before %s', async (operation) => {
    const hook = await mounted();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => command === 'load_session' ? Promise.resolve({
      ...initial, messages: [{ id: 'hidden', role: 'user', content: 'Hidden', timestamp: 0 }, ...initial.messages],
      artifacts: [{ id: 'hidden-artifact', type: 'text', title: 'Hidden', content: 'preserved', timestamp: 0 }],
    }) : backend(command, args));
    await act(async () => {
      if (operation === 'undo') await hook.result.current.undoTurn();
      if (operation === 'rollback') await hook.result.current.rollbackToMessage('u100');
      if (operation === 'edit') await hook.result.current.editAndResendMessage('u100', 'Edited');
    });
    expect(mocks.invoke).toHaveBeenCalledWith('load_session', { sessionId: 'one' });
    const save = mocks.invoke.mock.calls.find(([name]) => name === 'save_session')![1];
    expect(save.data.messages[0].id).toBe('hidden');
    expect(save.data.artifacts[0].id).toBe('hidden-artifact');
    expect(save.data.historyWindow).toBeUndefined();
  });
  it('rejects an empty visible text edit before full history hydration', async () => {
    const hook = await mounted();
    const messages = hook.result.current.messages;
    mocks.invoke.mockClear();
    await act(async () => { expect(await hook.result.current.editAndResendMessage('u100', '   ')).toBe(false); });
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(hook.result.current.messages).toBe(messages);
  });
  it('hydrates hidden artifacts even when every message is already visible', async () => {
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => {
      if (command === 'load_session_page') return Promise.resolve({ data: structuredClone(initial), window: { ...window, offset: 0, total: 2 } });
      if (command === 'load_session') return Promise.resolve({ ...structuredClone(initial), artifacts: [
        { id: 'hidden-artifact', type: 'text', title: 'Hidden', content: 'preserved', timestamp: 0 },
      ] });
      return backend(command, args);
    });
    const hook = await mounted();
    await act(async () => { await hook.result.current.undoTurn(); });
    expect(mocks.invoke).toHaveBeenCalledWith('load_session', { sessionId: 'one' });
    const saved = mocks.invoke.mock.calls.find(([name]) => name === 'save_session')![1].data;
    expect(saved.artifacts[0].id).toBe('hidden-artifact');
    expect(saved.historyWindow).toBeUndefined();
  });
  it('discards full hydration after navigating away', async () => {
    const hook = await mounted();
    const pending = deferred<SessionData>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => command === 'load_session' ? pending.promise : backend(command, args));
    let undo!: ReturnType<typeof hook.result.current.undoTurn>;
    act(() => { undo = hook.result.current.undoTurn(); });
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('load_session', { sessionId: 'one' }));
    await act(async () => { await hook.result.current.switchSession('two'); });
    await act(async () => { pending.resolve(initial); expect(await undo).toBeNull(); });
    expect(hook.result.current.currentSessionId).toBe('two');
    expect(hook.result.current.messages).toHaveLength(2);
  });
  it('loads a missing artifact independently of full history', async () => {
    const hook = await mounted();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => command === 'read_session_artifact' ? Promise.resolve({ id: 'hidden', type: 'text', title: 'Hidden', content: 'artifact', timestamp: 0 }) : backend(command, args));
    act(() => { hook.result.current.setActiveArtifactId('hidden'); });
    await waitFor(() => expect(hook.result.current.activeArtifact?.content).toBe('artifact'));
    expect(mocks.invoke).toHaveBeenCalledWith('read_session_artifact', { sessionId: 'one', artifactId: 'hidden' });
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'load_session')).toBe(false);
  });
  it('renames through the summary API without hydrating history', async () => {
    const hook = await mounted();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => command === 'rename_session_summary' ? Promise.resolve({ ...initial.session, title: args.title }) : backend(command, args));
    await act(async () => { await hook.result.current.renameSession('one', 'Renamed'); });
    expect(hook.result.current.sessionTitle).toBe('Renamed');
    expect(mocks.invoke).toHaveBeenCalledWith('rename_session_summary', { sessionId: 'one', title: 'Renamed' });
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'load_session')).toBe(false);
  });
  it('keeps the send cursor tied to its request snapshot while older history arrives during save', async () => {
    const hook = await mounted();
    const save = deferred<typeof window>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => {
      if (command === 'save_session_page') return save.promise;
      if (command === 'load_session_page' && args.before !== undefined) return Promise.resolve({ data: { ...initial, messages: [{ id: 'older', role: 'user', content: 'Old', timestamp: 1 }] }, window: { ...window, offset: 99, firstMessageId: 'older' } });
      return backend(command, args);
    });
    let sending!: Promise<boolean>;
    act(() => { sending = hook.result.current.sendMessage('Next'); });
    await waitFor(() => expect(mocks.invoke.mock.calls.some(([name]) => name === 'save_session_page')).toBe(true));
    await act(async () => { await hook.result.current.loadOlderMessages(); });
    await act(async () => { save.resolve({ ...window, revision: 'r2' }); await sending; });
    const request = mocks.invoke.mock.calls.find(([name]) => name === 'send_message')![1].request;
    expect(request.historyWindow.offset).toBe(100);
    expect(request.messages[0].id).toBe('u100');
    expect(hook.result.current.messages[0].id).toBe('older');
  });
  it('switches to a cached page while an older save is blocked and preserves newer config', async () => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.switchSession('two'); });
    await act(async () => { await hook.result.current.switchSession('one'); });
    const save = deferred<typeof window>();
    const backend = mocks.invoke.getMockImplementation()!;
    let saves = 0;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => command === 'save_session_page' && ++saves === 1 ? save.promise : backend(command, args));
    act(() => { hook.result.current.selectModel('p', 'older'); });
    await waitFor(() => expect(mocks.invoke.mock.calls.some(([name]) => name === 'save_session_page')).toBe(true));
    act(() => { hook.result.current.selectModel('p', 'newer'); });
    await act(async () => { await hook.result.current.switchSession('two'); });
    expect(hook.result.current.currentSessionId).toBe('two');
    await act(async () => { save.resolve({ ...window, revision: 'r2' }); });
    await act(async () => { await hook.result.current.switchSession('one'); });
    expect(hook.result.current.modelOverride?.model).toBe('newer');
    const windows = mocks.invoke.mock.calls.filter(([name]) => name === 'save_session_page').map(([, args]) => args.window);
    expect(windows[1].revision).toBe('r2');
  });
  it('does not let a queued review snapshot undo renamed metadata', async () => {
    const hook = await mounted();
    const rename = deferred<typeof initial.session>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => command === 'rename_session_summary' ? rename.promise : backend(command, args));
    let renaming!: Promise<void>;
    act(() => { renaming = hook.result.current.renameSession('one', 'Renamed'); });
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('rename_session_summary', { sessionId: 'one', title: 'Renamed' }));
    act(() => { mocks.listeners.get('agent:self-improvement')!({ payload: { session_id: 'one', run_id: 'a101', action: 'no_action', changes: [] } }); });
    await act(async () => { rename.resolve({ ...initial.session, title: 'Renamed' }); await renaming; });
    await waitFor(() => expect(mocks.invoke.mock.calls.some(([name]) => name === 'save_session_page')).toBe(true));
    const saved = mocks.invoke.mock.calls.filter(([name]) => name === 'save_session_page').at(-1)![1].data;
    expect(saved.session.title).toBe('Renamed');
    expect(hook.result.current.sessionTitle).toBe('Renamed');
  });
  it('restores only the latest page, not the full session', async () => {
    const hook = await mounted();
    expect(hook.result.current.currentSessionId).toBe('one');
    expect(hook.result.current.messages.map((m) => m.id)).toEqual(['u100', 'a101']);
    expect(mocks.invoke).toHaveBeenCalledWith('load_session_page', { sessionId: 'one', limit: 50 });
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'load_session')).toBe(false);
  });
});
