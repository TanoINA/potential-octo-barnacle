import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSettings, SelfImprovementEvent, SessionData, SessionWindow } from '../types/agent';
import { useAgent } from './useAgent';

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, (event: { payload: unknown }) => void>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
  mocks.listeners.set(name, handler);
  return () => mocks.listeners.delete(name);
}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));
const settings: AgentSettings = {
  providers: [{ id: 'p', name: 'Provider', kind: 'openai', baseUrl: 'http://localhost/v1', apiKey: '', model: 'model' }],
  activeProviderId: 'p', systemPrompt: '', temperature: 0.7, streamResponse: true, thinking: false,
  reasoningEffort: 'medium', favoriteModels: [], skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const initial: SessionData = {
  session: { id: 'one', title: 'Original', createdAt: 1, updatedAt: 2, messageCount: 102 },
  messages: [
    { id: 'u100', role: 'user', content: 'Prompt', timestamp: 100, isStreaming: false },
    { id: 'a101', role: 'assistant', content: 'Answer', timestamp: 101, runId: 'review-run', runStatus: 'completed', isStreaming: false,
      segments: [{ id: 'seg_0', text: 'Answer', toolCalls: [] }] },
  ],
  artifacts: [],
};
const cursor: SessionWindow = { offset: 100, total: 102, revision: 'r1', firstMessageId: 'u100' };
let canonical: SessionData;
let revision: number;
let accepted: number;
function currentWindow(): SessionWindow { return { ...cursor, revision: `r${revision}`, total: cursor.offset + canonical.messages.length }; }
function review(action: 'skill_staged' | 'skill_updated'): SelfImprovementEvent {
  return { session_id: 'one', run_id: 'review-run', action, changes: [{ action, pending_id: 'pending-one', skill_name: 'parser' }] };
}
async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  expect(hook.result.current.currentSessionId).toBe('one');
  return hook;
}
async function saveModel(hook: Awaited<ReturnType<typeof mounted>>) {
  await act(async () => { hook.result.current.selectModel('p', 'next-model'); });
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { promise, resolve };
}
beforeEach(() => {
  canonical = structuredClone(initial);
  revision = 1;
  accepted = 0;
  mocks.listeners.clear();
  mocks.invoke.mockReset();
  // Stateful native seam: metadata/review writes invalidate the revision. The
  // refresh contract validates the entire supplied tail, NOT just its first ID.
  mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [structuredClone(canonical.session)];
    if (command === 'load_session_page') return { data: structuredClone(canonical), window: currentWindow() };
    if (command === 'toggle_pin_session') {
      canonical.session.pinned = !canonical.session.pinned;
      canonical.session.pinnedAt = canonical.session.pinned ? 42 : undefined;
      revision++;
      return structuredClone(canonical.session);
    }
    if (command === 'rename_session_summary') {
      canonical.session.title = args.title;
      revision++;
      return structuredClone(canonical.session);
    }
    if (command === 'refresh_session_window') {
      expect(args.window.offset).toBe(100);
      expect(args.window.firstMessageId).toBe('u100');
      expect(args.data.historyWindow).toBeUndefined();
      const comparable = (data: SessionData) => ({
        messages: data.messages.map((message) => {
          if (args.reviewRunId && (message.runId === args.reviewRunId || message.id === args.reviewRunId)) {
            const { selfImprovement: _review, ...rest } = message;
            return rest;
          }
          return message;
        }),
        artifacts: data.artifacts,
        ...(!args.allowMetadata ? { session: data.session } : {}),
      });
      if (JSON.stringify(comparable(args.data)) !== JSON.stringify(comparable(canonical))) throw new Error('canonical content changed');
      return currentWindow();
    }
    if (command === 'save_session_page' || command === 'checkpoint_session_page') {
      if (args.window.revision !== `r${revision}`) throw new Error('stale cursor revision');
      const nativeReview = canonical.messages[1]?.selfImprovement;
      canonical = structuredClone(args.data);
      if (nativeReview) canonical.messages[1].selfImprovement = nativeReview;
      revision++;
      accepted++;
      return currentWindow();
    }
    return undefined;
  });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });


async function stream(hook: Awaited<ReturnType<typeof mounted>>, text: string) {
  const message = hook.result.current.messages.at(-1)!;
  let frame!: FrameRequestCallback;
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => { frame = callback; return 1; });
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => undefined);
  act(() => { mocks.listeners.get('agent-event')!({ payload: { type: 'chunk', data: { sessionId: 'one', runId: message.runId, text } } }); });
  act(() => { frame(0); });
}

describe('acknowledged revision races', () => {
  it('flushes the latest dirty stream before pinning and validates that acknowledged baseline', async () => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.sendMessage('Continue'); });
    await stream(hook, 'seed');
    await saveModel(hook);
    const seed = structuredClone(canonical);
    const saves = mocks.invoke.mock.calls.filter(([name]) => name === 'save_session_page').length;
    await stream(hook, ' advanced');
    await act(async () => { await hook.result.current.togglePinSession('one'); });
    expect(hook.result.current.lastError).toBeNull();
    const refresh = mocks.invoke.mock.calls.find(([name]) => name === 'refresh_session_window')![1];
    const flushed = mocks.invoke.mock.calls.filter(([name]) => name === 'save_session_page')[saves];
    const pinIndex = mocks.invoke.mock.calls.findIndex(([name]) => name === 'toggle_pin_session');
    expect(mocks.invoke.mock.calls.indexOf(flushed)).toBeLessThan(pinIndex);
    expect(flushed[1].data.messages.at(-1).content).toBe('seed advanced');
    expect(refresh.data.messages).toEqual(flushed[1].data.messages);
    expect(refresh.data.messages.at(-1).content).not.toBe(seed.messages.at(-1)!.content);
    expect(seed.messages.at(-1)!.content).toBe('seed');
    expect(hook.result.current.messages.at(-1)!.content).toBe('seed advanced');
    await saveModel(hook);
    expect(canonical.messages.at(-1)!.content).toBe('seed advanced');
    expect(hook.result.current.lastError).toBeNull();
  });
  it('uses the queued write acknowledgement even if a newer stream and review arrive before it resolves', async () => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.sendMessage('Continue'); });
    await stream(hook, 'queued seed');
    const gate = deferred<SessionWindow>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
      const result = await backend(command, args);
      return command === 'save_session_page' ? gate.promise : result;
    });
    act(() => { hook.result.current.selectModel('p', 'queued'); });
    await waitFor(() => expect(canonical.messages.at(-1)!.content).toBe('queued seed'));
    await stream(hook, ' newer');
    const payload = review('skill_updated');
    canonical.messages[1].selfImprovement = payload;
    revision++;
    act(() => { mocks.listeners.get('agent:self-improvement')!({ payload }); });
    mocks.invoke.mockImplementation(backend);
    await act(async () => { gate.resolve({ ...currentWindow(), revision: 'r3' }); });
    const refresh = mocks.invoke.mock.calls.find(([name]) => name === 'refresh_session_window')![1];
    expect(refresh.data.messages.at(-1).content).toBe('queued seed');
    expect(canonical.messages.at(-1)!.content).toBe('queued seed newer');
    expect(hook.result.current.lastError).toBeNull();
  });

  it('refreshes a cached page for an unloaded reviewed turn without writing it', async () => {
    const hook = await mounted();
    const payload = { ...review('skill_updated'), run_id: 'unloaded-run' };
    act(() => { hook.result.current.newSession(); });
    revision++;
    await act(async () => { mocks.listeners.get('agent:self-improvement')!({ payload }); });
    expect(mocks.invoke).toHaveBeenCalledWith('refresh_session_window', expect.objectContaining({ reviewRunId: 'unloaded-run' }));
    expect(accepted).toBe(0);
    await act(async () => { await hook.result.current.switchSession('one'); });
    await saveModel(hook);
    expect(accepted).toBe(1);
    expect(hook.result.current.lastError).toBeNull();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => {
      if (command === 'load_session_page' && args.before === 100) return {
        data: { ...structuredClone(initial), messages: [{ id: 'older-answer', role: 'assistant', content: 'Old answer', timestamp: 50, runId: 'unloaded-run', runStatus: 'completed', isStreaming: false }] },
        window: { offset: 99, total: 102, revision: currentWindow().revision, firstMessageId: 'older-answer' },
      };
      return backend(command, args);
    });
    await act(async () => { await hook.result.current.loadOlderMessages(); });
    expect(hook.result.current.messages[0].selfImprovement?.action).toBe('skill_updated');
  });

  it.each(['pin', 'rename'] as const)('does not report a delayed %s failure into a new session', async (kind) => {
    const hook = await mounted();
    const gate = deferred<SessionWindow>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => command === 'refresh_session_window' ? gate.promise : backend(command, args));
    let pending!: Promise<void>;
    act(() => { pending = kind === 'pin' ? hook.result.current.togglePinSession('one') : hook.result.current.renameSession('one', 'Renamed'); });
    await waitFor(() => expect(mocks.invoke.mock.calls.some(([name]) => name === 'refresh_session_window')).toBe(true));
    act(() => { hook.result.current.newSession(); });
    await act(async () => { gate.resolve(Promise.reject(new Error('old native failure')) as unknown as SessionWindow); await pending; });
    expect(hook.result.current.currentSessionId).not.toBe('one');
    expect(hook.result.current.lastError).toBeNull();
  });

  it('recovers a queued stale save after an already stored review without leaving a false error', async () => {
    const hook = await mounted();
    const payload = review('skill_updated');
    act(() => {
      hook.result.current.selectModel('p', 'before-event');
      canonical.messages[1].selfImprovement = payload;
      revision++;
      mocks.listeners.get('agent:self-improvement')!({ payload });
    });
    await waitFor(() => expect(accepted).toBe(1));
    expect(hook.result.current.lastError).toBeNull();
    expect(canonical.modelOverride?.model).toBe('before-event');
    expect(canonical.messages[1].selfImprovement?.action).toBe('skill_updated');
  });

  it.each(['pin', 'rename'] as const)('captures the acknowledged baseline inside the queued %s mutation', async (kind) => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.sendMessage('Continue'); });
    await stream(hook, 'saved by queue');
    const gate = deferred<SessionWindow>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
      const result = await backend(command, args);
      return command === 'save_session_page' ? gate.promise : result;
    });
    act(() => { hook.result.current.selectModel('p', 'queued'); });
    await waitFor(() => expect(canonical.messages.at(-1)!.content).toBe('saved by queue'));
    let pending!: Promise<void>;
    act(() => { pending = kind === 'pin' ? hook.result.current.togglePinSession('one') : hook.result.current.renameSession('one', 'Renamed'); });
    await stream(hook, ' newer');
    mocks.invoke.mockImplementation(backend);
    await act(async () => { gate.resolve(currentWindow()); await pending; });
    const refresh = mocks.invoke.mock.calls.find(([name]) => name === 'refresh_session_window')![1];
    expect(refresh.data.messages.at(-1).content).toBe('saved by queue');
    expect(hook.result.current.messages.at(-1)!.content).toBe('saved by queue newer');
    expect(hook.result.current.lastError).toBeNull();
  });

});
