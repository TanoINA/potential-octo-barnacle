import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent, AgentSettings, SelfImprovementEvent, SessionData } from '../types/agent';
import { useAgent } from './useAgent';
import { showDesktopNotification } from '../utils/audioChime';
import { open } from '@tauri-apps/plugin-dialog';

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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

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

  it('continues an offscreen run across switch and new, persists its events, and restores its live state', async () => {
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
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'abort_agent')).toBe(false);
    act(() => hook.result.current.newSession());
    const scope = { runId: request.runId, sessionId: 'sess-1' };
    act(() => {
      event({ type: 'chunk', data: { ...scope, text: 'Offscreen answer' } });
      event({ type: 'artifact', data: { ...scope, artifact: { id: 'term-a', title: 'Output', type: 'terminal', content: '', timestamp: 3 } } });
      event({ type: 'terminal_output', data: { ...scope, artifactId: 'term-a', delta: 'offscreen log', stream: 'stdout' } });
      event({ type: 'provider_state', data: { ...scope, providerMessages: [{ role: 'assistant', content: 'old snapshot' }] } });
      event({ type: 'provider_state', data: { ...scope, providerMessages: [{ role: 'assistant', content: [{ type: 'thinking', thinking: 'signed thought', signature: 'test-signature' }] }, { role: 'tool', tool_call_id: 'tool-a', content: 'result' }] } });
      event({ type: 'subagent_update', data: { ...scope, subagent: { id: 'sub-a', role: 'worker', task: 'test', status: 'running', startedAt: 3 } } });
    });
    await act(async () => { await hook.result.current.switchSession('sess-1'); });
    expect(hook.result.current.isStreaming).toBe(true);
    expect(hook.result.current.subagents[0]?.id).toBe('sub-a');
    act(() => event({ type: 'tool_update', data: { ...scope, tool: { id: 'running-tool', name: 'run_command', args: {}, status: 'done' } } }));
    await act(async () => { await hook.result.current.switchSession('sess-2'); });
    act(() => event({ type: 'done', data: { ...scope, cancelled: false, successful: true } }));
    await waitFor(() => expect(store.get('sess-1')?.messages.at(-1)?.runStatus).toBe('completed'));
    expect(store.get('sess-1')?.messages.at(-1)?.segments?.map((segment) => segment.text || '').join('')).toBe('Offscreen answer');
    expect(store.get('sess-1')?.artifacts[0]?.content).toBe('offscreen log');
    await act(async () => { await hook.result.current.switchSession('sess-1'); });
    expect(hook.result.current.messages.at(-1)?.runStatus).toBe('completed');
    expect(hook.result.current.messages.at(-1)?.providerMessages).toEqual([
      { role: 'assistant', content: [{ type: 'thinking', thinking: 'signed thought', signature: 'test-signature' }] },
      { role: 'tool', tool_call_id: 'tool-a', content: 'result' },
    ]);
    expect(request.workspaceCwd).toBe('D:/project');
    await act(async () => { await hook.result.current.reviewSession(); });
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'review_session')).toBe(true);
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

  it('keeps initialization loading and rejects sends until the restored session finishes loading', async () => {
    let resolveLoad!: (value: SessionData) => void;
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'get_settings') return structuredClone(settings);
      if (command === 'list_sessions') return [initial.session];
      if (command === 'load_session') return new Promise<SessionData>((resolve) => { resolveLoad = resolve; });
      return undefined;
    });
    const hook = renderHook(() => useAgent());
    await waitFor(() => expect(resolveLoad).toBeTypeOf('function'));
    expect(hook.result.current.isLoading).toBe(true);
    await act(async () => { expect(await hook.result.current.sendMessage('too early')).toBe(false); });
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'send_message')).toBe(false);
    await act(async () => { resolveLoad(structuredClone(initial)); });
    await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
    expect(hook.result.current.currentSessionId).toBe('sess-1');
  });

  it('routes concurrent council output to the origin while another viewed run can stop independently', async () => {
    const hook = await mounted();
    const store = new Map<string, SessionData>([['sess-1', structuredClone(initial)]]);
    mocks.invoke.mockImplementation(async (command: string, args: Record<string, unknown>) => {
      if (command === 'save_session') { const value = args.data as SessionData; store.set(value.session.id, structuredClone(value)); }
      if (command === 'load_session') return structuredClone(store.get(args.sessionId as string));
      if (command === 'list_sessions') return [...store.values()].map((value) => value.session);
      return undefined;
    });
    await act(async () => { await hook.result.current.sendMessage('compare', undefined, { council: true, candidateA: { provider }, candidateB: { provider: { ...provider, model: 'other-model' } } }); });
    const council = mocks.invoke.mock.calls.find(([name]) => name === 'send_council_message')?.[1];
    expect(council.requestA.workspaceCwd).toBe('D:/project');
    expect(council.requestB.workspaceCwd).toBe('D:/project');
    const scopeA = { runId: council.requestA.runId, sessionId: 'sess-1' };
    act(() => hook.result.current.newSession());
    const sessionB = hook.result.current.currentSessionId;
    await act(async () => { await hook.result.current.sendMessage('independent B'); });
    const requestB = mocks.invoke.mock.calls.filter(([name]) => name === 'send_message').at(-1)?.[1].request;
    expect(requestB.workspaceCwd).toBe('');
    act(() => {
      event({ type: 'council_chunk', data: { ...scopeA, candidate: 'A', kind: 'content', text: 'answer A' } });
      event({ type: 'council_chunk', data: { ...scopeA, candidate: 'B', kind: 'content', text: 'answer B' } });
      event({ type: 'council_done', data: { ...scopeA, candidate: 'A', durationMs: 10 } });
      event({ type: 'council_done', data: { ...scopeA, candidate: 'B', durationMs: 20 } });
      event({ type: 'done', data: { ...scopeA, cancelled: false } });
    });
    expect(hook.result.current.currentSessionId).toBe(sessionB);
    expect(hook.result.current.isStreaming).toBe(true);
    await act(async () => { await hook.result.current.stopAgent(); });
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'abort_agent').map(([, args]) => args.runId)).toEqual([requestB.runId]);
    act(() => event({ type: 'done', data: { runId: requestB.runId, sessionId: sessionB, cancelled: true } }));
    await act(async () => { await hook.result.current.switchSession('sess-1'); });
    expect(hook.result.current.messages.at(-1)?.councilTurn?.candidateA.content).toBe('answer A');
    expect(hook.result.current.messages.at(-1)?.councilTurn?.candidateB.content).toBe('answer B');
    expect(hook.result.current.isStreaming).toBe(false);
  });

  it('never lets a late origin start failure reset a different viewed run', async () => {
    const hook = await mounted();
    let rejectStart!: (error: Error) => void;
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'list_sessions') return [initial.session];
      if (command === 'send_message' && !rejectStart) return new Promise<void>((_, reject) => { rejectStart = reject; });
      return undefined;
    });
    let sending!: Promise<boolean>;
    act(() => { sending = hook.result.current.sendMessage('slow A'); });
    await waitFor(() => expect(rejectStart).toBeTypeOf('function'));
    act(() => hook.result.current.newSession());
    await act(async () => { await hook.result.current.sendMessage('fast B'); });
    const runB = mocks.invoke.mock.calls.filter(([name]) => name === 'send_message').at(-1)?.[1].request;
    await act(async () => { rejectStart(new Error('origin failed')); await sending; });
    expect(hook.result.current.isStreaming).toBe(true);
    act(() => event({ type: 'chunk', data: { runId: runB.runId, sessionId: runB.sessionId, text: 'B alive' } }));
    act(() => event({ type: 'done', data: { runId: runB.runId, sessionId: runB.sessionId, cancelled: false } }));
    expect(hook.result.current.messages.at(-1)?.content).toBe('B alive');
  });

  it('cancels a starting run before delayed persistence can launch it', async () => {
    const hook = await mounted();
    let resolveSave!: () => void;
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'save_session' && !resolveSave) return new Promise<void>((resolve) => { resolveSave = resolve; });
      if (command === 'list_sessions') return [initial.session];
      return undefined;
    });
    let sending!: Promise<boolean>;
    act(() => { sending = hook.result.current.sendMessage('pending start'); });
    await waitFor(() => expect(resolveSave).toBeTypeOf('function'));
    await act(async () => { await hook.result.current.stopAgent(); });
    await act(async () => { resolveSave(); await sending; });
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'send_message')).toBe(false);
    expect(hook.result.current.messages.at(-1)?.runStatus).toBe('cancelled');
    expect(hook.result.current.isStreaming).toBe(false);
    expect(hook.result.current.stopRequested).toBe(true);
    act(() => hook.result.current.updateRunConfig({ thinking: true }));
    expect(hook.result.current.stopRequested).toBe(true);
    await act(async () => { await hook.result.current.sendMessage('explicit resume'); });
    expect(hook.result.current.stopRequested).toBe(false);
    act(() => hook.result.current.updateRunConfig({ thinking: false }));
    expect(hook.result.current.stopRequested).toBe(false);
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'send_message')).toHaveLength(1);
  });

  it('retries with fresh buffers and never resends an edited prompt into a newly viewed session', async () => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.sendMessage('old run'); });
    const oldRun = mocks.invoke.mock.calls.find(([name]) => name === 'send_message')?.[1].request;
    act(() => event({ type: 'chunk', data: { runId: oldRun.runId, sessionId: 'sess-1', text: 'old buffered text' } }));
    let resolveSave!: () => void;
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'save_session' && !resolveSave) return new Promise<void>((resolve) => { resolveSave = resolve; });
      if (command === 'list_sessions') return [initial.session];
      return undefined;
    });
    let editing!: Promise<boolean>;
    act(() => { editing = hook.result.current.editAndResendMessage('user-1', 'edited A'); });
    await waitFor(() => expect(resolveSave).toBeTypeOf('function'));
    act(() => hook.result.current.newSession());
    await act(async () => { resolveSave(); expect(await editing).toBe(false); });
    expect(hook.result.current.messages).toHaveLength(0);
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'send_message')).toHaveLength(1);
  });

  it('preserves compacted history through the next live mutation', async () => {
    const hook = await mounted();
    const compacted = structuredClone(initial);
    compacted.messages = [{ id: 'summary', role: 'system', content: 'summary only', timestamp: 3 }];
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'list_sessions') return [initial.session];
      if (command === 'compact_session') return compacted;
      return undefined;
    });
    await act(async () => { await hook.result.current.sendMessage('one more'); });
    const run = mocks.invoke.mock.calls.find(([name]) => name === 'send_message')?.[1].request;
    act(() => event({ type: 'done', data: { runId: run.runId, sessionId: 'sess-1', cancelled: false } }));
    await act(async () => { await hook.result.current.compactSession(); });
    act(() => hook.result.current.updateRunConfig({ thinking: true }));
    expect(hook.result.current.messages.map((message) => message.id)).toEqual(['summary']);
  });

  it('keeps completed offscreen state authoritative while final persistence is blocked', async () => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.sendMessage('offscreen'); });
    const run = mocks.invoke.mock.calls.find(([name]) => name === 'send_message')?.[1].request;
    let release!: () => void;
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'save_session' && !release) return new Promise<void>((resolve) => { release = resolve; });
      if (command === 'list_sessions') return [initial.session];
      if (command === 'load_session') return structuredClone(initial);
      return undefined;
    });
    act(() => hook.result.current.newSession());
    await waitFor(() => expect(release).toBeTypeOf('function'));
    act(() => {
      event({ type: 'chunk', data: { runId: run.runId, sessionId: 'sess-1', text: 'final answer' } });
      event({ type: 'done', data: { runId: run.runId, sessionId: 'sess-1', cancelled: false } });
    });
    await act(async () => { await hook.result.current.switchSession('sess-1'); });
    expect(hook.result.current.messages.at(-1)?.content).toBe('final answer');
    expect(hook.result.current.messages.at(-1)?.runStatus).toBe('completed');
    await act(async () => release());
  });

  it('restores the first artifact and retains explicit selection through live publication', async () => {
    const restored = structuredClone(initial);
    restored.artifacts = [
      { id: 'new', title: 'New', type: 'code', content: '', timestamp: 3 },
      { id: 'old', title: 'Old', type: 'code', content: '', timestamp: 2 },
    ];
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'get_settings') return structuredClone(settings);
      if (command === 'list_sessions') return [restored.session];
      if (command === 'load_session') return restored;
      return undefined;
    });
    const hook = await mounted();
    expect(hook.result.current.activeArtifactId).toBe('new');
    act(() => hook.result.current.setActiveArtifactId('old'));
    act(() => hook.result.current.updateRunConfig({ thinking: true }));
    expect(hook.result.current.activeArtifactId).toBe('old');
    expect(hook.result.current.activeArtifact?.id).toBe('old');
  });

  it('serializes settings saves and rolls back only the latest revision to confirmed settings', async () => {
    const hook = await mounted();
    const firstSave = deferred<void>();
    const secondSave = deferred<void>();
    const saves = [firstSave, secondSave];
    let saveCount = 0;
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'save_settings' && saveCount < saves.length) return saves[saveCount++].promise;
      return undefined;
    });
    const first = { ...settings, temperature: 0.1 };
    const second = { ...settings, temperature: 0.2 };
    let savingFirst!: Promise<unknown>;
    let savingSecond!: Promise<unknown>;
    act(() => {
      savingFirst = hook.result.current.updateSettings(first).catch((error) => error);
      savingSecond = hook.result.current.updateSettings(second).catch((error) => error);
    });
    await waitFor(() => expect(saveCount).toBeGreaterThan(0));
    expect(saveCount).toBe(1);
    await act(async () => { firstSave.reject(new Error('first failed')); await savingFirst; });
    expect(hook.result.current.settings?.temperature).toBe(0.2);
    await waitFor(() => expect(saveCount).toBe(2));
    await act(async () => { secondSave.reject(new Error('second failed')); await savingSecond; });
    expect(hook.result.current.settings).toEqual(settings);
    expect(hook.result.current.lastError).toContain('second failed');
    await act(async () => { await hook.result.current.updateSettings({ ...settings, temperature: 0.3 }); });
    expect(hook.result.current.settings?.temperature).toBe(0.3);
  });

  it('merges fetched models into current settings instead of the pre-fetch snapshot', async () => {
    const hook = await mounted();
    const fetched = deferred<string[]>();
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'fetch_provider_models') return fetched.promise;
      return undefined;
    });
    let refreshing!: Promise<string[]>;
    act(() => { refreshing = hook.result.current.refreshModels(provider.id); });
    await act(async () => { await hook.result.current.updateSettings({ ...settings, temperature: 0.25, favoriteModels: ['new-favorite'] }); });
    await act(async () => { fetched.resolve(['remote-model']); await refreshing; });
    expect(hook.result.current.settings).toMatchObject({ temperature: 0.25, favoriteModels: ['new-favorite'] });
    expect(hook.result.current.settings?.providers[0].models).toEqual(['remote-model']);
  });

  it('does not select a custom model into a session navigated to during settings persistence', async () => {
    const hook = await mounted();
    const saved = deferred<void>();
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'save_settings') return saved.promise;
      if (command === 'list_sessions') return [initial.session];
      return undefined;
    });
    let adding!: Promise<void>;
    act(() => { adding = hook.result.current.addCustomModel(provider.id, 'custom-model'); });
    act(() => hook.result.current.newSession());
    await act(async () => { saved.resolve(); await adding; });
    expect(hook.result.current.modelOverride).toBeNull();
    expect(hook.result.current.settings?.providers[0].customModels).toEqual(['custom-model']);
  });

  it('discards a workspace dialog resolved after navigation', async () => {
    const hook = await mounted();
    const dialog = deferred<string>();
    vi.mocked(open).mockReturnValueOnce(dialog.promise);
    let selecting!: Promise<void>;
    act(() => { selecting = hook.result.current.selectWorkspace(); });
    act(() => hook.result.current.newSession());
    await act(async () => { dialog.resolve('D:/late'); await selecting; });
    expect(hook.result.current.workspaceCwd).toBe('');
    expect(mocks.invoke.mock.calls.some(([name, args]) => name === 'set_workspace_cwd' && args?.newCwd === 'D:/late')).toBe(false);
  });

  it.each(['select', 'clear'] as const)('persists %s workspace metadata on the origin without changing the new session', async (operation) => {
    const hook = await mounted();
    const changed = deferred<string>();
    vi.mocked(open).mockResolvedValueOnce('D:/selected');
    let started = false;
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'set_workspace_cwd' && !started) { started = true; return changed.promise; }
      if (command === 'list_sessions') return [initial.session];
      if (command === 'load_session') return structuredClone(initial);
      return undefined;
    });
    let changing!: Promise<void>;
    act(() => { changing = operation === 'select' ? hook.result.current.selectWorkspace() : hook.result.current.clearWorkspace(); });
    await waitFor(() => expect(started).toBe(true));
    act(() => hook.result.current.newSession());
    const nextId = hook.result.current.currentSessionId;
    await act(async () => { changed.resolve(operation === 'select' ? 'D:/selected' : ''); await changing; });
    expect(hook.result.current.currentSessionId).toBe(nextId);
    expect(hook.result.current.workspaceCwd).toBe('');
    const expected = operation === 'select' ? 'D:/selected' : undefined;
    const originSaves = mocks.invoke.mock.calls.filter(([name, args]) => name === 'save_session' && args.data.session.id === 'sess-1');
    expect(originSaves.at(-1)?.[1].data.session.workspaceCwd).toBe(expected);
    await act(async () => { await hook.result.current.switchSession('sess-1'); });
    expect(hook.result.current.workspaceCwd).toBe(expected ?? '');
  });

  it.each(['listAllArtifacts', 'deepSearch'] as const)('propagates %s errors so callers can display Retry', async (operation) => {
    const hook = await mounted();
    const failure = new Error('index unavailable');
    mocks.invoke.mockRejectedValue(failure);
    await act(async () => {
      const request = operation === 'listAllArtifacts' ? hook.result.current.listAllArtifacts() : hook.result.current.deepSearch('parser');
      await expect(request).rejects.toBe(failure);
    });
    expect(hook.result.current.lastError).toContain('index unavailable');
  });

  it('binds memory callbacks to the explicit or synchronously captured workspace', async () => {
    const hook = await mounted();
    await act(async () => {
      await hook.result.current.readAgentsMd();
      await hook.result.current.saveAgentsMd('memory');
      await hook.result.current.initAgentsMd();
    });
    expect(mocks.invoke).toHaveBeenCalledWith('read_workspace_agents_md', { expectedWorkspace: 'D:/project' });
    expect(mocks.invoke).toHaveBeenCalledWith('save_workspace_agents_md', { content: 'memory', expectedWorkspace: 'D:/project' });
    expect(mocks.invoke).toHaveBeenCalledWith('init_workspace_agents_md', { expectedWorkspace: 'D:/project' });
    act(() => hook.result.current.newSession());
    await act(async () => {
      await hook.result.current.readAgentsMd('D:/origin');
      await hook.result.current.saveAgentsMd('origin memory', 'D:/origin');
      await hook.result.current.initAgentsMd('D:/origin');
    });
    expect(mocks.invoke).toHaveBeenCalledWith('read_workspace_agents_md', { expectedWorkspace: 'D:/origin' });
    expect(mocks.invoke).toHaveBeenCalledWith('save_workspace_agents_md', { content: 'origin memory', expectedWorkspace: 'D:/origin' });
    expect(mocks.invoke).toHaveBeenCalledWith('init_workspace_agents_md', { expectedWorkspace: 'D:/origin' });
  });

  it('renames the current session through native metadata and retains messages', async () => {
    const hook = await mounted();
    const renamed = structuredClone(initial);
    renamed.session.title = 'Native title';
    renamed.messages = [];
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'rename_session') return renamed;
      if (command === 'list_sessions') return [renamed.session];
      return undefined;
    });
    await act(async () => { await hook.result.current.renameSession('sess-1', 'Native title'); });
    expect(mocks.invoke).toHaveBeenCalledWith('rename_session', { sessionId: 'sess-1', title: 'Native title' });
    expect(hook.result.current.sessionTitle).toBe('Native title');
    expect(hook.result.current.messages).toHaveLength(2);
  });

  it.each(['rename', 'pin'] as const)('updates authoritative offscreen %s metadata without replacing live messages', async (operation) => {
    const hook = await mounted();
    act(() => hook.result.current.updateRunConfig({ thinking: true }));
    const updated = structuredClone(initial);
    updated.session.title = 'Origin renamed';
    updated.session.pinned = true;
    updated.session.pinnedAt = 10;
    updated.messages = [];
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === 'rename_session') return updated;
      if (command === 'toggle_pin_session') return updated.session;
      if (command === 'list_sessions') return [updated.session];
      if (command === 'load_session') return structuredClone(initial);
      return undefined;
    });
    act(() => hook.result.current.newSession());
    await act(async () => {
      if (operation === 'rename') await hook.result.current.renameSession('sess-1', 'Origin renamed');
      else await hook.result.current.togglePinSession('sess-1');
    });
    await act(async () => { await hook.result.current.switchSession('sess-1'); });
    expect(hook.result.current.messages).toHaveLength(2);
    if (operation === 'rename') expect(hook.result.current.sessionTitle).toBe('Origin renamed');
    else {
      act(() => hook.result.current.updateRunConfig({ thinking: false }));
      await waitFor(() => {
        const saved = mocks.invoke.mock.calls.filter(([name]) => name === 'save_session').at(-1)?.[1].data as SessionData;
        expect(saved.session).toMatchObject({ pinned: true, pinnedAt: 10 });
      });
    }
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
