import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSettings, SessionData, SessionPage } from '../types/agent';
import { useAgent } from './useAgent';

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, (event: { payload: unknown }) => void>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
  mocks.listeners.set(name, handler);
  return () => mocks.listeners.delete(name);
}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));
const settings = {
  providers: [{ id: 'p', name: 'Provider', kind: 'openai', baseUrl: 'http://localhost/v1', apiKey: '', model: 'model' }],
  activeProviderId: 'p', systemPrompt: '', temperature: 0.7, streamResponse: true, thinking: false,
  reasoningEffort: 'medium', favoriteModels: [], skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
} as AgentSettings;
let canonical: SessionData;
let revision: string;
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}
function page(args: Record<string, any>): SessionPage {
  if (args.revision && args.revision !== revision) throw new Error('Session page cursor is stale; reload the latest page');
  const end = args.before ?? canonical.messages.length;
  const offset = Math.max(0, end - Math.min(100, args.limit));
  const data = structuredClone({ ...canonical, messages: canonical.messages.slice(offset, end) });
  return { data, window: { offset, total: canonical.messages.length, revision, firstMessageId: data.messages[0]?.id ?? null } };
}
function externalEdit(count = 800) {
  canonical = {
    ...canonical, session: { ...canonical.session, title: 'External title', messageCount: count },
    messages: Array.from({ length: count }, (_, i) => ({ id: 'm' + i, role: 'user' as const, content: 'Canonical edit ' + i, timestamp: i })),
    artifacts: [{ id: 'artifact', type: 'code', title: 'Output', content: 'Canonical artifact', timestamp: 1 }],
  };
  revision = 'external';
}
async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  return hook;
}
async function stale(hook: Awaited<ReturnType<typeof mounted>>) {
  await act(async () => { await hook.result.current.loadOlderMessages(); });
  expect(hook.result.current.historyError).toMatch(/retry/i);
}
const saves = () => mocks.invoke.mock.calls.filter(([name]) => name === 'save_session_page' || name === 'checkpoint_session_page');
const reads = () => mocks.invoke.mock.calls.filter(([name]) => name === 'load_session_page');
beforeEach(() => {
  revision = 'original';
  canonical = {
    session: { id: 'one', title: 'Original', createdAt: 1, updatedAt: 2, messageCount: 800 },
    messages: Array.from({ length: 800 }, (_, i) => ({ id: 'm' + i, role: 'user', content: 'Original ' + i, timestamp: i })),
    artifacts: [{ id: 'artifact', type: 'code', title: 'Output', content: 'Original artifact', timestamp: 1 }],
  };
  mocks.listeners.clear();
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [canonical.session];
    if (command === 'load_session_page') return page(args);
    if (command === 'save_session_page' || command === 'checkpoint_session_page') {
      if (args.window.revision !== revision) throw new Error('stale write');
      return { ...args.window, total: args.window.offset + args.data.messages.length };
    }
    return undefined;
  });
});
afterEach(cleanup);

describe('history conflict recovery', () => {
  it.each([800, 810, 730])('atomically reloads the loaded suffix and older page from canonical history (%i messages)', async (count) => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.loadOlderMessages(); await hook.result.current.loadOlderMessages(); });
    expect(hook.result.current.messages[0].id).toBe('m650');
    externalEdit(count);
    const before = hook.result.current.messages;
    await stale(hook);
    expect(hook.result.current.messages).toBe(before);
    expect(reads()).toHaveLength(4); // No automatic retry after the stale read.
    expect(saves()).toHaveLength(0);
    await act(async () => { await hook.result.current.loadOlderMessages(); });
    const offset = Math.max(0, Math.min(650, count - 150) - 50);
    expect(hook.result.current.historyError).toBeNull();
    expect(hook.result.current.messages.map(({ id, content }) => ({ id, content }))).toEqual(canonical.messages.slice(offset).map(({ id, content }) => ({ id, content })));
    expect(hook.result.current.messages.some((message) => message.id === 'm650')).toBe(true);
    expect(hook.result.current.sessionTitle).toBe('External title');
    expect(hook.result.current.totalMessageCount).toBe(count);
    expect(hook.result.current.hasOlderMessages).toBe(true);
    expect(saves()).toHaveLength(0);
    expect(reads().slice(4)[0][1]).toEqual({ sessionId: 'one', limit: 50 });
    expect(reads().slice(5).every(([, args]) => args.limit <= 50 && args.revision === 'external')).toBe(true);
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'load_session')).toBe(false);
    act(() => { hook.result.current.selectModel('p', 'next'); });
    await waitFor(() => expect(saves()).toHaveLength(1));
    const saved = saves()[0][1];
    expect(saved.window).toMatchObject({ offset, revision: 'external' });
    expect(saved.data.messages.map(({ content }: { content: string }) => content)).toEqual(canonical.messages.slice(offset).map(({ content }) => content));
    expect(saved.data.artifacts[0].content).toBe('Canonical artifact');
  });

  it.each(['dirty', 'running', 'saving'] as const)('does not rebase a %s session or perform extra saves on retry', async (state) => {
    const hook = await mounted();
    const backend = mocks.invoke.getMockImplementation()!;
    if (state === 'running') await act(async () => { await hook.result.current.sendMessage('Local run'); });
    if (state !== 'running') {
      mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => {
        if (command === 'save_session_page') return state === 'saving' ? new Promise(() => {}) : Promise.reject(new Error('Local save failed'));
        return backend(command, args);
      });
      act(() => { hook.result.current.selectModel('p', 'local'); });
      await waitFor(() => expect(saves()).toHaveLength(1));
      if (state === 'dirty') await waitFor(() => expect(hook.result.current.lastError).toMatch(/Local save failed/));
    }
    externalEdit();
    await stale(hook);
    const before = hook.result.current.messages;
    const readCount = reads().length;
    const saveCount = saves().length;
    await act(async () => { await hook.result.current.loadOlderMessages(); });
    expect(hook.result.current.historyError).toMatch(/finish or stop.*reload.*before saving/i);
    expect(hook.result.current.messages).toBe(before);
    expect(reads()).toHaveLength(readCount);
    expect(saves()).toHaveLength(saveCount);
    // A later attempted write must still carry the old revision; native rejects it.
    if (state !== 'saving') {
      mocks.invoke.mockImplementation(backend);
      act(() => { hook.result.current.selectModel('p', 'another'); });
      await waitFor(() => expect(saves()).toHaveLength(saveCount + 1));
      expect(saves().at(-1)![1].window.revision).toBe('original');
    }
  });

  it.each(['read failure', 'local mutation', 'navigation'] as const)('discards the whole recovery after %s', async (cause) => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.loadOlderMessages(); });
    externalEdit();
    await stale(hook);
    const before = hook.result.current.messages;
    const pending = deferred<SessionPage>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => {
      if (command === 'load_session_page' && args.before !== undefined && args.revision === 'external') return pending.promise;
      return backend(command, args);
    });
    let recovery!: Promise<void>;
    act(() => { recovery = hook.result.current.loadOlderMessages(); });
    await waitFor(() => expect(reads().at(-1)![1].revision).toBe('external'));
    if (cause === 'local mutation') act(() => { hook.result.current.selectModel('p', 'local'); });
    if (cause === 'navigation') act(() => { hook.result.current.newSession(); });
    await act(async () => {
      if (cause === 'read failure') {
        revision = 'newer';
        // Return a different revision to exercise consistency checking even if a backend returns rather than throws.
      }
      pending.resolve(page({ before: 750, limit: 50 }));
      await recovery;
    });
    if (cause === 'navigation') expect(hook.result.current.currentSessionId).not.toBe('one');
    else {
      expect(hook.result.current.messages).toBe(before);
      expect(hook.result.current.historyError).not.toBeNull();
      mocks.invoke.mockImplementation(backend);
      const saveCount = saves().length;
      act(() => { hook.result.current.selectModel('p', 'after'); });
      await waitFor(() => expect(saves()).toHaveLength(saveCount + 1));
      expect(saves().at(-1)![1].window.revision).toBe('original');
    }
  });
});

describe('recovered canonical persistence baseline', () => {
  function nativeMetadataBackend() {
    const backend = mocks.invoke.getMockImplementation()!;
    const state = { accepted: 0 };
    mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
      if (command === 'toggle_pin_session') {
        canonical.session = { ...canonical.session, pinned: !canonical.session.pinned, pinnedAt: 42 };
        revision += ':pin';
        return structuredClone(canonical.session);
      }
      if (command === 'refresh_session_window') {
        const { window, data } = args;
        // Native validates the entire canonical suffix and total, not merely
        // the boundary ID or a revision string supplied by the UI.
        if (window.total !== canonical.messages.length
          || window.firstMessageId !== canonical.messages[window.offset]?.id
          || JSON.stringify(data.messages) !== JSON.stringify(canonical.messages.slice(window.offset))
          || JSON.stringify(data.artifacts) !== JSON.stringify(canonical.artifacts)) {
          throw new Error('canonical suffix content or total changed');
        }
        return { ...window, revision };
      }
      if (command === 'save_session_page' || command === 'checkpoint_session_page') {
        if (args.window.revision !== revision) throw new Error('stale cursor revision');
        canonical = structuredClone({ ...args.data, messages: [...canonical.messages.slice(0, args.window.offset), ...args.data.messages] });
        revision += ':save';
        state.accepted++;
        return { ...args.window, revision, total: canonical.messages.length };
      }
      return backend(command, args);
    });
    return state;
  }

  it.each([
    { count: 800, overlay: false }, { count: 810, overlay: false },
    { count: 800, overlay: true }, { count: 810, overlay: true },
  ])('pins and saves after explicit recovery ($count messages, cached review: $overlay)', async ({ count, overlay }) => {
    canonical.messages = canonical.messages.map((message) => ({ ...message, isStreaming: false }));
    const native = nativeMetadataBackend();
    const hook = await mounted();
    const payload = { session_id: 'one', run_id: 'recovered-run', action: 'skill_updated' as const, changes: [] };
    if (overlay) {
      // The reviewed turn is not loaded yet: cache the IPC payload without
      // editing or persisting the visible suffix.
      await act(async () => { mocks.listeners.get('agent:self-improvement')!({ payload }); });
      expect(saves()).toHaveLength(0);
    }
    externalEdit(count);
    if (overlay) canonical.messages[700] = {
      ...canonical.messages[700], role: 'assistant', runId: payload.run_id, runStatus: 'completed',
      selfImprovement: { ...payload, action: 'no_action' },
    };
    await stale(hook);
    const readCount = reads().length;
    expect(saves()).toHaveLength(0);
    await act(async () => { await hook.result.current.loadOlderMessages(); });
    expect(reads().length).toBeGreaterThan(readCount);
    expect(hook.result.current.historyError).toBeNull();
    expect(hook.result.current.totalMessageCount).toBe(count);
    expect(hook.result.current.messages[0].id).toBe('m700');
    if (overlay) expect(hook.result.current.messages[0].selfImprovement).toEqual(payload);
    const recoveredCanonical = structuredClone({ ...canonical, messages: canonical.messages.slice(700) });
    const refreshCount = mocks.invoke.mock.calls.filter(([name]) => name === 'refresh_session_window').length;
    await act(async () => { await hook.result.current.togglePinSession('one'); });
    const refreshes = mocks.invoke.mock.calls.filter(([name]) => name === 'refresh_session_window');
    expect(refreshes).toHaveLength(refreshCount + 1);
    const baseline = refreshes.at(-1)![1];
    expect(baseline.window).toEqual({ offset: 700, total: count, revision: 'external', firstMessageId: 'm700' });
    expect(baseline.data).toEqual(recoveredCanonical);
    expect(baseline.data.historyWindow).toBeUndefined();
    if (overlay) expect(baseline.data.messages[0].selfImprovement.action).toBe('no_action');
    expect(hook.result.current.lastError).toBeNull();
    await act(async () => { hook.result.current.selectModel('p', 'after-recovery'); });
    await waitFor(() => expect(native.accepted).toBe(1));
    expect(hook.result.current.lastError).toBeNull();
    expect(canonical.modelOverride?.model).toBe('after-recovery');
    expect(canonical.session.pinned).toBe(true);
    expect(saves()).toHaveLength(1);
    expect(saves()[0][1].window).toMatchObject({ offset: 700, total: count, revision: 'external:pin' });
    expect(canonical.messages[700].content).toBe('Canonical edit 700');
    if (overlay) expect(canonical.messages[700].selfImprovement).toEqual(payload);
  });

  it('retains the acknowledged shorter suffix for an ordinary older-page load', async () => {
    canonical.messages = canonical.messages.map((message) => ({ ...message, isStreaming: false }));
    const native = nativeMetadataBackend();
    const hook = await mounted();
    const acknowledged = structuredClone({ ...canonical, messages: canonical.messages.slice(750) });
    await act(async () => { await hook.result.current.loadOlderMessages(); });
    expect(hook.result.current.messages[0].id).toBe('m700');
    await act(async () => { await hook.result.current.togglePinSession('one'); });
    const baseline = mocks.invoke.mock.calls.find(([name]) => name === 'refresh_session_window')![1];
    expect(baseline.window).toEqual({ offset: 750, total: 800, revision: 'original', firstMessageId: 'm750' });
    expect(baseline.data).toEqual(acknowledged);
    expect(hook.result.current.lastError).toBeNull();
    await act(async () => { hook.result.current.selectModel('p', 'after-older-load'); });
    await waitFor(() => expect(native.accepted).toBe(1));
    expect(hook.result.current.lastError).toBeNull();
    expect(canonical.messages).toHaveLength(800);
  });
});
