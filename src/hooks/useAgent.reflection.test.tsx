import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent, AgentSettings, SelfImprovementEvent, SessionData } from '../types/agent';
import { useAgent } from './useAgent';
import { showDesktopNotification } from '../utils/audioChime';

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    mocks.listeners.set(name, handler);
    return () => mocks.listeners.delete(name);
  }),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));

const provider = { id: 'provider', name: 'Provider', kind: 'openai' as const, baseUrl: 'http://localhost:1234/v1', apiKey: '', model: 'base-model' };
const settings: AgentSettings = {
  providers: [provider], activeProviderId: provider.id, systemPrompt: '', temperature: 0.7,
  streamResponse: true, thinking: false, reasoningEffort: 'medium', favoriteModels: [],
  skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const initial: SessionData = {
  session: { id: 'sess-1', title: 'Parser', createdAt: 1, updatedAt: 2, messageCount: 2, workspaceCwd: 'D:/project' },
  messages: [
    { id: 'user-1', role: 'user', content: 'Fix parser', timestamp: 1 },
    { id: 'assistant-1', role: 'assistant', content: 'Fixed parser', timestamp: 2, runId: 'old-run', runStatus: 'completed' },
  ], artifacts: [], modelOverride: { providerId: provider.id, model: 'session-model' },
};

async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.messages).toHaveLength(2));
  return hook;
}
function event(payload: AgentEvent) { mocks.listeners.get('agent-event')?.({ payload }); }
function review(payload: SelfImprovementEvent) { mocks.listeners.get('agent:self-improvement')?.({ payload }); }

beforeEach(() => {
  mocks.listeners.clear();
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(async (command: string) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [initial.session];
    if (command === 'load_session') return structuredClone(initial);
    return undefined;
  });
});

describe('useAgent reflection', () => {
  it.each(['/review', '/reflect'])('routes %s without adding chat messages and uses the session provider', async (command) => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.sendMessage(command); });
    const call = mocks.invoke.mock.calls.find(([name]) => name === 'review_session');
    expect(call).toBeTruthy();
    expect(call?.[1]).toEqual({ sessionId: 'sess-1', provider: { ...provider, model: 'session-model' } });
    expect(hook.result.current.messages).toHaveLength(2);
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'send_message')).toBe(false);
    const saveIndex = mocks.invoke.mock.calls.findIndex(([name]) => name === 'save_session');
    const reviewIndex = mocks.invoke.mock.calls.findIndex(([name]) => name === 'review_session');
    expect(saveIndex).toBeLessThan(reviewIndex);
  });

  it('retains a delayed result on its origin after done and during the next run', async () => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.sendMessage('First follow-up'); });
    const first = mocks.invoke.mock.calls.find(([name]) => name === 'send_message')?.[1].request;
    expect(first.self_improvement_enabled).toBe(false);
    act(() => event({ type: 'done', data: { runId: first.runId, sessionId: 'sess-1', cancelled: false } }));
    await act(async () => { await hook.result.current.sendMessage('Second follow-up'); });
    const payload: SelfImprovementEvent = {
      run_id: first.runId, session_id: 'sess-1', action: 'skill_created', skill_name: 'parser-recipe',
    };
    act(() => review(payload));
    expect(hook.result.current.messages.find((message) => message.runId === first.runId)?.selfImprovement).toEqual(payload);
    expect(hook.result.current.messages.at(-1)?.selfImprovement).toBeUndefined();
    act(() => review({ ...payload, session_id: 'other-session', skill_name: 'wrong-session' }));
    expect(hook.result.current.messages.find((message) => message.runId === first.runId)?.selfImprovement).toEqual(payload);
    await waitFor(() => expect(mocks.invoke.mock.calls.some(([name, args]) => name === 'save_session' && args.data.messages.some((message: { selfImprovement?: SelfImprovementEvent }) => message.selfImprovement?.skill_name === 'parser-recipe'))).toBe(true));
  });

  it('blocks reflection for failed and cancelled completed streams', async () => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.sendMessage('Follow-up'); });
    const request = mocks.invoke.mock.calls.find(([name]) => name === 'send_message')?.[1].request;
    act(() => event({ type: 'done', data: { runId: request.runId, sessionId: 'sess-1', cancelled: false, successful: false } }));
    await act(async () => { await hook.result.current.sendMessage('/review'); });
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'review_session')).toBe(false);
    expect(hook.result.current.messages.at(-1)?.runStatus).toBe('failed');
  });

  it('persists cancelled status when switching away from an active run and refuses its review on return', async () => {
    const hook = await mounted();
    const store = new Map<string, SessionData>([['sess-1', structuredClone(initial)]]);
    const other = structuredClone(initial);
    other.session.id = 'sess-2';
    store.set('sess-2', other);
    mocks.invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
      if (command === 'save_session') { const data = args.data as SessionData; store.set(data.session.id, structuredClone(data)); }
      if (command === 'load_session') return structuredClone(store.get(args.sessionId as string));
      if (command === 'list_sessions') return [...store.values()].map((data) => data.session);
      return undefined;
    });
    await act(async () => { await hook.result.current.sendMessage('A long task'); });
    const request = mocks.invoke.mock.calls.find(([name]) => name === 'send_message')?.[1].request;
    act(() => event({ type: 'tool_update', data: { runId: request.runId, sessionId: 'sess-1', tool: { id: 'running-tool', name: 'run_command', args: {}, status: 'running' } } }));
    await act(async () => { await hook.result.current.switchSession('sess-2'); });
    await act(async () => { await hook.result.current.switchSession('sess-1'); });
    expect(hook.result.current.messages.at(-1)?.runStatus).toBe('cancelled');
    expect(hook.result.current.messages.at(-1)?.toolCalls?.[0].status).toBe('cancelled');
    await act(async () => { await hook.result.current.reviewSession(); });
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'review_session')).toBe(false);
  });

  it('merges a review event that arrives after a stale session load starts but before it resolves', async () => {
    const hook = await mounted();
    const other = structuredClone(initial);
    other.session.id = 'sess-2';
    let resolveLoad!: (data: SessionData) => void;
    mocks.invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
      if (command === 'list_sessions') return [other.session, initial.session];
      if (command === 'load_session' && args.sessionId === 'sess-2') return other;
      if (command === 'load_session' && args.sessionId === 'sess-1') return new Promise<SessionData>((resolve) => { resolveLoad = resolve; });
      return undefined;
    });
    await act(async () => { await hook.result.current.switchSession('sess-2'); });
    let navigation!: Promise<void>;
    act(() => { navigation = hook.result.current.switchSession('sess-1'); });
    await waitFor(() => expect(resolveLoad).toBeTypeOf('function'));
    const payload: SelfImprovementEvent = { run_id: 'old-run', session_id: 'sess-1', action: 'skill_created', skill_name: 'review-race-recipe' };
    act(() => review(payload));
    await act(async () => { resolveLoad(structuredClone(initial)); await navigation; });
    expect(hook.result.current.messages[1].selfImprovement).toEqual(payload);
  });

  it('never starts review when persisting the completed turn fails', async () => {
    const hook = await mounted();
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'save_session') throw new Error('disk full');
      if (command === 'list_sessions') return [initial.session];
      return undefined;
    });
    await act(async () => { await hook.result.current.sendMessage('/review'); });
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'review_session')).toBe(false);
    expect(hook.result.current.lastError).toContain('disk full');
  });

  it('never previews the user prompt in the completion notification', async () => {
    const hidden = Object.getOwnPropertyDescriptor(document, 'hidden');
    Object.defineProperty(document, 'hidden', { configurable: true, value: true });
    const now = vi.spyOn(Date, 'now');
    now.mockReturnValue(1_000);
    try {
      const hook = await mounted();
      await act(async () => { await hook.result.current.sendMessage('Summarize the repo'); });
      const runId = mocks.invoke.mock.calls.find(([name]) => name === 'send_message')?.[1].request.runId as string;
      // Failed run with no assistant output: the notification must not echo the user prompt.
      now.mockReturnValue(10_000);
      act(() => event({ type: 'done', data: { runId, sessionId: 'sess-1', cancelled: false, successful: false } }));
      expect(showDesktopNotification).toHaveBeenCalledWith(
        'AetherAgent: Task Complete',
        expect.not.stringContaining('Summarize the repo'),
      );
    } finally {
      now.mockRestore();
      if (hidden) Object.defineProperty(document, 'hidden', hidden);
      else Object.defineProperty(document, 'hidden', { configurable: true, value: false });
    }
  });
});
