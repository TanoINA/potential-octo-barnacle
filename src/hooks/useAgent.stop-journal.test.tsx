import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent, SessionData } from '../types/agent';
import { pagingBackend } from '../test/sessionPaging';
import { pruneSessionCache } from '../utils/sessionCacheBudget';
import * as cacheBudget from '../utils/sessionCacheBudget';
import * as liveSessions from './useAgentLiveSession';
import { useAgent } from './useAgent';
const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, (event: { payload: any }) => void>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name: string, handler: any) => { mocks.listeners.set(name, handler); return () => mocks.listeners.delete(name); }) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));
const provider = { id: 'p', name: 'P', kind: 'openai' as const, baseUrl: '', apiKey: '', model: 'm' };
const initial: SessionData = { session: { id: 'origin', title: 'Origin', createdAt: 1, updatedAt: 1, messageCount: 0 }, messages: [], artifacts: [] };
let canonical: SessionData;
let journal: SessionData['messages'] | undefined;
let failInitial: boolean;
let holdSave: Promise<void> | undefined;
const calls = (name: string) => mocks.invoke.mock.calls.filter(([command]) => command === name).map(([, args]) => args);
function event(payload: AgentEvent) { mocks.listeners.get('agent-event')!({ payload }); }
function assertTerminalTools(messages: SessionData['messages']) {
  // Stateful IPC double of native terminal immutability, including segment copies.
  for (const previous of journal ?? []) {
    const next = messages.find(message => message.id === previous.id)!;
    const tools = (message: typeof previous) => [...(message.toolCalls ?? []), ...(message.segments ?? []).flatMap(segment => segment.toolCalls ?? [])];
    for (const tool of tools(previous)) if (['done', 'error', 'cancelled'].includes(tool.status)) {
      if (JSON.stringify(tools(next).find(item => item.id === tool.id)) !== JSON.stringify(tool)) throw new Error('terminal tool changed after journal checkpoint');
    }
  }
}
beforeEach(() => {
  canonical = structuredClone(initial); journal = undefined; failInitial = false; holdSave = undefined;
  mocks.listeners.clear(); mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(pagingBackend(async (command: string, args: any) => {
    if (command === 'get_settings') return { providers: [provider], activeProviderId: 'p', skillPaths: [], mcpServers: [], maxTurns: 5, self_improvement_enabled: false };
    if (command === 'list_sessions') return [canonical.session];
    if (command === 'load_session') return structuredClone(canonical);
    if (command === 'save_session') {
      await holdSave;
      if (failInitial) { failInitial = false; throw new Error('initial disk failure'); }
      assertTerminalTools(args.data.messages); canonical = structuredClone(args.data); journal = undefined;
    }
    if (command === 'checkpoint_session_delta') { assertTerminalTools(args.request.messages); journal = structuredClone(args.request.messages); }
  }));
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); });
async function ready() {
  vi.spyOn(cacheBudget, 'scheduleSessionIndexWarmup');
  const factory = vi.spyOn(liveSessions, 'liveSession');
  const hook = renderHook(useAgent);
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  const live = factory.mock.results.map(result => result.value as liveSessions.LiveSession).find(value => value.data.session.id === 'origin')!;
  return { hook, live };
}
function timers() {
  vi.spyOn(window, 'requestAnimationFrame').mockReturnValue(99);
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => undefined);
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
}
async function tick() { await act(async () => { await vi.advanceTimersByTimeAsync(2000); }); }
const councilOptions = { council: true, candidateA: { provider }, candidateB: { provider } };
describe('stop journal persistence', () => {
  it.each(['done', 'cancelled'] as const)('retains running sidecar until native %s enrichment and final cancelled save', async status => {
    const { hook, live } = await ready();
    await act(async () => { await hook.result.current.sendMessage('work'); });
    const scope = { sessionId: 'origin', runId: live.runId! }; timers();
    const tool = { id: 'tool', name: 'run_command', args: { command: 'work' }, status: 'running' as const, startedAt: 10 };
    act(() => event({ type: 'tool_update', data: { ...scope, tool } })); await tick();
    expect(journal?.[0].toolCalls?.[0].status).toBe('running');
    act(() => event({ type: 'chunk', data: { ...scope, text: 'trailing progress' } }));
    await act(async () => { await hook.result.current.stopAgent(); });
    expect(hook.result.current.messages.at(-1)?.toolCalls?.[0].status).toBe('cancelled'); await tick();
    const enriched = { ...tool, status, result: 'native trailing result', exitCode: 0, completedAt: 20 };
    act(() => event({ type: 'tool_update', data: { ...scope, tool: enriched } })); await tick();
    await act(async () => event({ type: 'done', data: { ...scope, cancelled: true } }));
    expect(hook.result.current.lastError).toBeNull();
    expect(hook.result.current.runStatus).toMatchObject({ phase: 'cancelled' });
    expect(canonical.messages.at(-1)).toMatchObject({ runStatus: 'cancelled', isStreaming: false, toolCalls: [enriched] });
    expect(journal).toBeUndefined(); expect(calls('checkpoint_session_delta')).toHaveLength(1); expect(live.checkpointTimer).toBeNull();
  });
  it('drops a queued checkpoint when Stop precedes IPC submission', async () => {
    const { hook, live } = await ready();
    await act(async () => { await hook.result.current.sendMessage('work'); });
    const scope = { sessionId: 'origin', runId: live.runId! }; timers();
    act(() => event({ type: 'tool_update', data: { ...scope, tool: { id: 't', name: 'read_file', args: {}, status: 'running' } } })); await tick();
    let release!: () => void; holdSave = new Promise<void>(resolve => { release = resolve; });
    act(() => { hook.result.current.updateRunConfig({ thinking: true }); });
    act(() => event({ type: 'chunk', data: { ...scope, text: 'queued progress' } })); await tick();
    expect(live.deltaBusy).toBe(true);
    await act(async () => { await hook.result.current.stopAgent(); });
    await act(async () => { release(); await holdSave; });
    expect(calls('checkpoint_session_delta')).toHaveLength(1); await tick(); expect(live.checkpointTimer).toBeNull();
    await act(async () => event({ type: 'done', data: { ...scope, cancelled: true } })); expect(hook.result.current.lastError).toBeNull();
  });
});
describe('failed startup ownership', () => {
  it.each([false, true])('clears starting on initial save failure (council=%s), permitting eviction, idle and retry', async council => {
    const { hook, live } = await ready(); timers(); failInitial = true;
    await act(async () => { expect(await hook.result.current.sendMessage('fail', undefined, council ? councilOptions : undefined)).toBe(true); });
    expect(live.starting).toBe(false); expect(live.runId).toBeNull(); expect(live.dirty).toBe(false);
    expect(pruneSessionCache(new Map([['origin', live]]), 'elsewhere', 0, 0)).toEqual(['origin']);
    expect(vi.mocked(cacheBudget.scheduleSessionIndexWarmup).mock.calls.at(-1)![0]()).toBe(true);
    await act(async () => { expect(await hook.result.current.sendMessage('retry', undefined, council ? councilOptions : undefined)).toBe(true); });
    expect(calls(council ? 'send_council_message' : 'send_message')).toHaveLength(1); expect(hook.result.current.isStreaming).toBe(true);
  });
  it.each([false, true])('does not publish offscreen initial save failure into viewed session (council=%s)', async council => {
    const { hook, live } = await ready(); const backend = mocks.invoke.getMockImplementation()!;
    let rejectSave!: (error: Error) => void;
    mocks.invoke.mockImplementation(async (command: string, args: any) => {
      if (command === 'save_session_page' && !rejectSave) return new Promise((_, reject) => { rejectSave = reject; });
      return backend(command, args);
    });
    let sending!: Promise<boolean>;
    act(() => { sending = hook.result.current.sendMessage('origin', undefined, council ? councilOptions : undefined); });
    await waitFor(() => expect(rejectSave).toBeTypeOf('function')); act(() => hook.result.current.newSession());
    const viewed = hook.result.current.currentSessionId; const messages = hook.result.current.messages;
    await act(async () => { rejectSave(new Error('offscreen disk failure')); await sending; });
    expect(hook.result.current.currentSessionId).toBe(viewed); expect(hook.result.current.messages).toBe(messages);
    expect(hook.result.current.lastError).toBeNull(); expect(hook.result.current.runStatus).toBeNull(); expect(live.starting).toBe(false);
    await act(async () => { expect(await hook.result.current.sendMessage('viewed retry')).toBe(true); });
  });
});

it.each([false, true])('does not clear a newer same-session startup after a stale provider rejection (council=%s)', async council => {
  const { hook, live } = await ready();
  const backend = mocks.invoke.getMockImplementation()!;
  let rejectProvider!: (error: Error) => void;
  const commandName = council ? 'send_council_message' : 'send_message';
  mocks.invoke.mockImplementation(async (command: string, args: any) => {
    if (command === commandName && !rejectProvider) return new Promise((_, reject) => { rejectProvider = reject; });
    return backend(command, args);
  });
  let origin!: Promise<boolean>;
  act(() => { origin = hook.result.current.sendMessage('origin', undefined, council ? councilOptions : undefined); });
  await waitFor(() => expect(rejectProvider).toBeTypeOf('function'));
  const oldRun = live.runId!;
  await act(async () => event({ type: 'done', data: { sessionId: 'origin', runId: oldRun, cancelled: false } }));
  let release!: () => void;
  holdSave = new Promise<void>(resolve => { release = resolve; });
  let newer!: Promise<boolean>;
  act(() => { newer = hook.result.current.sendMessage('newer'); });
  const newRun = live.runId;
  expect(newRun).not.toBe(oldRun); expect(live.starting).toBe(true);
  await act(async () => { rejectProvider(new Error('stale provider rejection')); await origin; });
  expect(live.runId).toBe(newRun); expect(live.starting).toBe(true); expect(hook.result.current.lastError).toBeNull();
  await act(async () => { release(); await newer; });
  expect(live.starting).toBe(false); expect(live.runId).toBe(newRun);
});