import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent, AgentSettings, SessionData, SessionWindow } from '../types/agent';
import { useAgent } from './useAgent';

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, (event: { payload: any }) => void>() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name: string, handler: any) => {
  mocks.listeners.set(name, handler); return () => mocks.listeners.delete(name);
}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));
const provider = { id: 'p', name: 'Provider', kind: 'openai' as const, baseUrl: 'http://localhost/v1', apiKey: '', model: 'model' };
const settings: AgentSettings = {
  providers: [provider], activeProviderId: 'p', systemPrompt: '', temperature: 0.7, streamResponse: true,
  thinking: false, reasoningEffort: 'medium', favoriteModels: [], skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const image = 'HISTORICAL_BASE64_' + 'x'.repeat(8192);
let canonical: SessionData;
let journal: { messages: SessionData['messages']; artifacts: SessionData['artifacts'] } | undefined;
let revision: number;
let failDelta: boolean;
let fresh: boolean;
let sequence: number;
let holdDelta: Promise<void> | undefined;
const calls = (name: string) => mocks.invoke.mock.calls.filter(([command]) => command === name).map(([, args]) => args);
function windowFor(offset: number): SessionWindow {
  return { offset, total: canonical.messages.length, revision: `r${revision}`, firstMessageId: canonical.messages[offset]?.id ?? null };
}
function fold() {
  if (!journal) return;
  canonical.messages = canonical.messages.map((m) => journal!.messages.find((next) => next.id === m.id) ?? m);
  const artifacts = new Map(canonical.artifacts.map((a) => [a.id, a]));
  for (const a of journal.artifacts) artifacts.set(a.id, a);
  canonical.artifacts = [...artifacts.values()]; journal = undefined; revision++;
}
function event(payload: AgentEvent) { mocks.listeners.get('agent-event')!({ payload }); }
async function started(council = false) {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  await act(async () => { expect(await hook.result.current.sendMessage('new prompt', undefined, council ? {
    council: true, candidateA: { provider }, candidateB: { provider },
  } : undefined)).toBe(true); });
  const scope = { sessionId: hook.result.current.currentSessionId, runId: hook.result.current.messages.at(-1)!.runId! };
  vi.spyOn(window, 'requestAnimationFrame').mockReturnValue(99);
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => undefined);
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
  return { hook, scope };
}
async function tick() { await act(async () => { await vi.advanceTimersByTimeAsync(2000); }); }
beforeEach(() => {
  revision = 1; sequence = 0; journal = undefined; failDelta = false; fresh = false; holdDelta = undefined;
  canonical = {
    session: { id: 'one', title: 'History', createdAt: 1, updatedAt: 2, messageCount: 200 },
    messages: Array.from({ length: 200 }, (_, i) => ({ id: `m${i}`, role: i % 2 ? 'assistant' : 'user', content: `history ${i}`, timestamp: i,
      ...(i % 2 ? {} : { imageInputs: [{ sourcePath: 'history.png', data: image, mediaType: 'image/png' }] }),
    })),
    artifacts: [{ id: 'old', title: 'Unchanged', type: 'code', content: 'old artifact', timestamp: 1 }],
  };
  mocks.listeners.clear(); mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(async (command: string, args: any) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return fresh ? [] : [structuredClone(canonical.session)];
    if (command === 'load_session_page') {
      fold();
      if (args.revision) expect(args.revision).toBe(`r${revision}`);
      const end = args.before ?? canonical.messages.length;
      const offset = Math.max(0, end - args.limit);
      return { data: { ...structuredClone(canonical), messages: structuredClone(canonical.messages.slice(offset, end)) }, window: windowFor(offset) };
    }
    if (command === 'save_session_page' || command === 'save_session') {
      if (args.window) expect(args.window.revision).toBe(`r${revision}`);
      assertVersions(journal ?? canonical, args.data);
      const prefix = args.window ? canonical.messages.slice(0, args.window.offset) : [];
      canonical = { ...structuredClone(args.data), messages: [...prefix, ...structuredClone(args.data.messages)] };
      journal = undefined; revision++;
      return args.window ? windowFor(args.window.offset) : undefined;
    }
    if (command === 'checkpoint_session_delta') {
      await holdDelta;
      if (failDelta) { failDelta = false; throw new Error('temporary disk failure'); }
      expect(args.request.baseRevision).toBe(`r${revision}`);
      expect(args.request.sequence).toBeGreaterThan(sequence); sequence = args.request.sequence;
      assertVersions(journal ?? canonical, args.request);
      const artifacts = new Map(journal?.artifacts.map((a) => [a.id, a]));
      for (const a of args.request.artifacts) artifacts.set(a.id, structuredClone(a));
      journal = { messages: structuredClone(args.request.messages), artifacts: [...artifacts.values()] };
      return { revision: `r${revision}`, sequence };
    }
    if (command === 'toggle_pin_session' || command === 'rename_session_summary') {
      fold(); revision++;
      if (command === 'toggle_pin_session') canonical.session.pinned = !canonical.session.pinned;
      else canonical.session.title = args.title;
      return structuredClone(canonical.session);
    }
    if (command === 'refresh_session_window') {
      expect(args.data.messages).toEqual(canonical.messages.slice(args.window.offset));
      expect(args.data.artifacts).toEqual(canonical.artifacts);
      return windowFor(args.window.offset);
    }
    return undefined;
  });
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); });

// Stateful IPC double: checks serialized lifecycle counters, not the native validator.
// Native session_journal tests independently cover the real storage contract.
function assertVersions(old: Pick<SessionData, 'messages' | 'artifacts'>, next: Pick<SessionData, 'messages' | 'artifacts'>) {
  for (const message of next.messages) {
    const previous = old.messages.find((m) => m.id === message.id);
    if (!previous) continue;
    expect(message.checkpointEpoch ?? 0).toBeGreaterThanOrEqual(previous.checkpointEpoch ?? 0);
    if (!(message.reasoning ?? '').startsWith(previous.reasoning ?? '')) {
      expect(message.checkpointEpoch ?? 0).toBeGreaterThan(previous.checkpointEpoch ?? 0);
    }
    expect((message.preamble ?? '') + message.content).toContain((previous.preamble ?? '') + previous.content);
    for (const tool of previous.toolCalls ?? []) expect(message.toolCalls?.some((t) => t.id === tool.id)).toBe(true);
  }
  for (const artifact of next.artifacts) {
    const previous = old.artifacts.find((a) => a.id === artifact.id);
    if (!previous) continue;
    for (const key of ['id', 'title', 'type', 'language', 'filePath', 'timestamp'] as const) expect(artifact[key]).toEqual(previous[key]);
    expect(artifact.streamRevision ?? 0).toBeGreaterThanOrEqual(previous.streamRevision ?? 0);
    if (!artifact.content.startsWith(previous.content)) expect(artifact.streamRevision ?? 0).toBeGreaterThan(previous.streamRevision ?? 0);
  }
}

describe('journal lifecycle versions', () => {
  it('checkpoints old reasoning then retry reasoning without losing narrative or tools, and preserves the epoch on final save', async () => {
    const { hook, scope } = await started();
    act(() => {
      event({ type: 'chunk', data: { ...scope, text: 'retained opening' } });
      event({ type: 'tool_update', data: { ...scope, tool: { id: 'tool', name: 'read_file', args: {}, status: 'done', result: 'read' } } });
      event({ type: 'chunk', data: { ...scope, text: 'retained answer' } });
      event({ type: 'reasoning', data: { ...scope, text: 'old reasoning' } });
    });
    await tick();
    const first = calls('checkpoint_session_delta')[0].request.messages[0];
    expect(first.reasoning).toBe('old reasoning');
    expect(first.checkpointEpoch ?? 0).toBe(0);
    act(() => {
      event({ type: 'notice', data: { ...scope, message: 'waiting', resetPartial: false } });
    });
    expect(hook.result.current.messages.at(-1)?.checkpointEpoch ?? 0).toBe(0);
    act(() => {
      event({ type: 'chunk', data: { ...scope, text: ' buffered narrative' } });
      event({ type: 'reasoning', data: { ...scope, text: ' abandoned buffered reasoning' } });
      event({ type: 'notice', data: { ...scope, message: 'Retrying', resetPartial: true, attempt: 2 } });
    });
    expect(hook.result.current.messages.at(-1)).toMatchObject({ checkpointEpoch: 1, reasoning: '', content: 'retained answer buffered narrative', preamble: 'retained opening', toolCalls: first.toolCalls, segments: first.segments.map((segment: any, index: number) => index === first.segments.length - 1 ? { ...segment, text: segment.text + ' buffered narrative' } : segment) });
    act(() => event({ type: 'reasoning', data: { ...scope, text: 'new reasoning' } }));
    await tick();
    expect(calls('checkpoint_session_delta')).toHaveLength(2);
    expect(journal?.messages[0]).toMatchObject({ reasoning: 'new reasoning', checkpointEpoch: 1 });
    await act(async () => event({ type: 'done', data: { ...scope, cancelled: false } }));
    expect(canonical.messages.at(-1)).toMatchObject({ reasoning: 'new reasoning', checkpointEpoch: 1, content: 'retained answer buffered narrative', isStreaming: false });
    expect(journal).toBeUndefined();
    expect(hook.result.current.lastError).toBeNull();
  });

  it('checkpoints non-prefix artifact replacements and saves a newer replacement immediately before the next tick', async () => {
    const { hook, scope } = await started();
    const artifact = { id: 'output', title: 'Output', type: 'code' as const, language: 'ts', filePath: 'output.ts', timestamp: 5, content: 'draft body' };
    act(() => event({ type: 'artifact', data: { ...scope, artifact } }));
    await tick();
    expect(journal?.artifacts[0]).toMatchObject({ content: 'draft body', streamRevision: 1 });
    act(() => event({ type: 'artifact', data: { ...scope, artifact: { ...artifact, content: 'final body' } } }));
    await tick();
    expect(journal?.artifacts[0]).toMatchObject({ content: 'final body', streamRevision: 2 });
    await act(async () => {
      event({ type: 'artifact', data: { ...scope, artifact: { ...artifact, content: 'saved immediately' } } });
      event({ type: 'done', data: { ...scope, cancelled: false } });
    });
    expect(canonical.artifacts.find((a) => a.id === artifact.id)).toEqual({ ...artifact, content: 'saved immediately', streamRevision: 3 });
    expect(calls('checkpoint_session_delta')).toHaveLength(2);
    expect(journal).toBeUndefined();
    expect(hook.result.current.lastError).toBeNull();
  });

  it('keeps terminal appends at their event revision and does not version lazy hydration', async () => {
    const historical = { id: 'lazy', title: 'Historical', type: 'code' as const, content: 'historical body', timestamp: 4, streamRevision: 7 };
    canonical.artifacts.push(historical);
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(async (command: string, args: any) => {
      if (command === 'read_session_artifact') return structuredClone(historical);
      const result = await backend(command, args);
      if (command === 'load_session_page') result.data.artifacts = result.data.artifacts.filter((a: any) => a.id !== historical.id);
      return result;
    });
    const { hook, scope } = await started();
    await act(async () => hook.result.current.setActiveArtifactId(historical.id));
    expect(hook.result.current.artifacts.find((a) => a.id === historical.id)).toEqual(historical);
    act(() => {
      event({ type: 'artifact', data: { ...scope, artifact: { id: 'terminal', title: 'Terminal', type: 'terminal', content: '', timestamp: 5 } } });
      event({ type: 'terminal_output', data: { ...scope, artifactId: 'terminal', delta: 'first', stream: 'stdout' } });
    });
    await tick();
    expect(journal?.artifacts).toEqual([expect.objectContaining({ id: 'terminal', content: 'first', streamRevision: 1 })]);
    act(() => event({ type: 'terminal_output', data: { ...scope, artifactId: 'terminal', delta: ' second', stream: 'stdout' } }));
    await tick();
    expect(journal?.artifacts[0]).toMatchObject({ content: 'first second', streamRevision: 1 });
    await act(async () => event({ type: 'done', data: { ...scope, cancelled: false } }));
    expect(canonical.artifacts.find((a) => a.id === 'terminal')).toMatchObject({ streamRevision: 1, content: 'first second' });
    expect(hook.result.current.lastError).toBeNull();
  });
});
