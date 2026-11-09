import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent, AgentSettings, SessionData, SessionWindow } from '../types/agent';
import { useAgent } from './useAgent';
import * as liveSessions from './useAgentLiveSession';

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
let sequence: Map<string, number>;
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
  revision = 1; sequence = new Map(); journal = undefined; failDelta = false; fresh = false; holdDelta = undefined;
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
      const prefix = args.window ? canonical.messages.slice(0, args.window.offset) : [];
      canonical = { ...structuredClone(args.data), messages: [...prefix, ...structuredClone(args.data.messages)] };
      journal = undefined; revision++;
      return args.window ? windowFor(args.window.offset) : undefined;
    }
    if (command === 'checkpoint_session_delta') {
      await holdDelta;
      if (failDelta) { failDelta = false; throw new Error('temporary disk failure'); }
      expect(args.request.baseRevision).toBe(`r${revision}`);
      expect(args.request.sequence).toBeGreaterThan(sequence.get(args.request.runId) ?? 0); sequence.set(args.request.runId, args.request.sequence);
      const artifacts = new Map(journal?.artifacts.map((a) => [a.id, a]));
      for (const a of args.request.artifacts) artifacts.set(a.id, structuredClone(a));
      journal = { messages: structuredClone(args.request.messages), artifacts: [...artifacts.values()] };
      return { revision: `r${revision}`, sequence: args.request.sequence };
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

describe('incremental live checkpoint IPC', () => {
  it('owns only run-changed artifact IDs across lazy reads, pending acknowledgements, metadata saves and the next run', async () => {
    const large = { id: 'historical-large', title: 'Historical body', type: 'code' as const, content: 'OLD_LARGE_BODY_' + 'z'.repeat(65536), timestamp: 1 };
    canonical.artifacts.push(large);
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(async (command: string, args: any) => {
      if (command === 'read_session_artifact') return structuredClone(large);
      if (command === 'refresh_session_window') {
        expect(args.data.messages).toEqual(canonical.messages.slice(args.window.offset));
        const byId = (artifacts: SessionData['artifacts']) => [...artifacts].sort((a, b) => a.id.localeCompare(b.id));
        // Native artifact validation is ID-based; persisted map order is not UI order.
        expect(byId(args.data.artifacts)).toEqual(byId(canonical.artifacts));
        return windowFor(args.window.offset);
      }
      const retained = command === 'save_session_page' ? canonical.artifacts : [];
      const response = await backend(command, args);
      if (command === 'load_session_page') response.data.artifacts = response.data.artifacts.filter((a: any) => a.id !== large.id);
      if (command === 'save_session_page') {
        const merged = new Map(retained.map((a) => [a.id, a]));
        for (const a of canonical.artifacts) merged.set(a.id, a);
        canonical.artifacts = [...merged.values()];
      }
      return response;
    });
    const factory = vi.spyOn(liveSessions, 'liveSession');
    const { hook, scope } = await started(true);
    const live = factory.mock.results.map((r) => r.value as liveSessions.LiveSession).find((value) => value.data.session.id === scope.sessionId)!;
    await act(async () => { hook.result.current.setActiveArtifactId(large.id); });
    expect(hook.result.current.artifacts.find((a) => a.id === large.id)?.content).toBe(large.content);
    const terminal = { id: 'terminal', type: 'terminal' as const, title: 'Terminal', content: '', timestamp: 2 };
    const council = { id: 'council', type: 'code' as const, title: 'Council output', content: 'candidate body', timestamp: 3 };
    act(() => {
      event({ type: 'artifact', data: { ...scope, artifact: terminal } });
      event({ type: 'artifact', data: { ...scope, artifact: council } });
      event({ type: 'terminal_output', data: { ...scope, artifactId: terminal.id, delta: 'first', stream: 'stdout' } });
      event({ type: 'council_chunk', data: { ...scope, candidate: 'A', kind: 'content', text: 'candidate' } });
    });
    await tick();
    const first = calls('checkpoint_session_delta')[0].request;
    expect(first.artifacts.map((a: any) => a.id).sort()).toEqual(['council', 'terminal']);
    expect(JSON.stringify(first)).not.toContain('OLD_LARGE_BODY_');
    expect(first.messages[0].councilTurn.candidateA.content).toBe('candidate');
    expect(live.checkpointArtifactIds.has(large.id)).toBe(false);
    // A current artifact event may legitimately regenerate an already loaded ID.
    const regenerated = { ...canonical.artifacts.find((a) => a.id === 'old')!, content: 'regenerated' };
    let release!: () => void;
    holdDelta = new Promise<void>((resolve) => { release = resolve; });
    act(() => {
      event({ type: 'artifact', data: { ...scope, artifact: regenerated } });
      event({ type: 'artifact', data: { ...scope, artifact: { ...council, content: 'second' } } });
      event({ type: 'terminal_output', data: { ...scope, artifactId: terminal.id, delta: ' second', stream: 'stdout' } });
    });
    await tick();
    act(() => {
      event({ type: 'artifact', data: { ...scope, artifact: { ...council, content: 'third' } } });
      event({ type: 'terminal_output', data: { ...scope, artifactId: terminal.id, delta: ' third', stream: 'stdout' } });
    });
    await act(async () => { release(); await holdDelta; });
    await tick();
    expect(journal?.artifacts.find((a) => a.id === council.id)?.content).toBe('third');
    expect(journal?.artifacts.find((a) => a.id === terminal.id)?.content).toBe('first second third');
    expect(journal?.artifacts.find((a) => a.id === 'old')?.content).toBe('regenerated');
    let enterMetadata!: () => void;
    const metadataEntered = new Promise<void>((resolve) => { enterMetadata = resolve; });
    let releaseMetadata!: () => void;
    const metadataGate = new Promise<void>((resolve) => { releaseMetadata = resolve; });
    const metadataBackend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(async (command: string, args: any) => {
      if (command === 'toggle_pin_session') { enterMetadata(); await metadataGate; }
      return metadataBackend(command, args);
    });
    let pin!: Promise<void>;
    act(() => { pin = hook.result.current.togglePinSession(scope.sessionId); });
    await act(async () => { await metadataEntered; });
    act(() => event({ type: 'artifact', data: { ...scope, artifact: { ...council, content: 'during metadata' } } }));
    await act(async () => { releaseMetadata(); await pin; });
    expect(live.checkpointArtifactIds.has(council.id)).toBe(true);
    await tick();
    expect(journal?.artifacts.find((a) => a.id === council.id)?.content).toBe('during metadata');
    expect(calls('checkpoint_session_delta').every((call: any) => call.request.artifacts.every((a: any) => a.id !== large.id))).toBe(true);
    await act(async () => { event({ type: 'done', data: { ...scope, cancelled: false } }); });
    expect(canonical.artifacts.find((a) => a.id === large.id)?.content).toBe(large.content);
    expect(canonical.artifacts.find((a) => a.id === terminal.id)?.content).toBe('first second third');
    expect(canonical.artifacts.find((a) => a.id === council.id)?.content).toBe('during metadata');
    await act(async () => { await hook.result.current.sendMessage('next run'); });
    expect(live.checkpointArtifactIds.size).toBe(0);
    const nextScope = { sessionId: scope.sessionId, runId: hook.result.current.messages.at(-1)!.runId! };
    act(() => event({ type: 'chunk', data: { ...nextScope, text: 'next answer' } }));
    await tick();
    expect(calls('checkpoint_session_delta').at(-1).request.artifacts).toEqual([]);
    expect(hook.result.current.lastError).toBeNull();
  });

  it('journals only the current assistant and changed artifacts without a frame or historical images', async () => {
    const { hook, scope } = await started(true);
    const writes = calls('save_session_page').length;
    act(() => {
      event({ type: 'artifact', data: { ...scope, artifact: { id: 'terminal', type: 'terminal', title: 'Terminal', content: '', timestamp: 1 } } });
      event({ type: 'chunk', data: { ...scope, text: 'partial' } });
      event({ type: 'council_chunk', data: { ...scope, candidate: 'A', kind: 'content', text: 'candidate' } });
      event({ type: 'terminal_output', data: { ...scope, artifactId: 'terminal', delta: 'output', stream: 'stdout' } });
    });
    await tick();
    const first = calls('checkpoint_session_delta')[0];
    expect(first).toBeDefined();
    expect(first.request.messages).toHaveLength(1);
    expect(first.request.messages[0]).toMatchObject({ role: 'assistant', runId: scope.runId, content: 'partial', councilTurn: { candidateA: { content: 'candidate' } } });
    expect(first.request.artifacts.map((a: any) => a.id)).toEqual(['terminal']);
    expect(JSON.stringify(first)).not.toContain('HISTORICAL_BASE64');
    expect(JSON.stringify(first)).not.toContain('imageInputs');
    expect(calls('save_session_page')).toHaveLength(writes);
    expect(calls('checkpoint_session_page')).toHaveLength(0);
    expect(canonical.messages.at(-1)?.content).toBe('');
    act(() => event({ type: 'chunk', data: { ...scope, text: ' more' } }));
    await tick();
    expect(calls('checkpoint_session_delta')[1].request).toMatchObject({ sequence: 2, baseRevision: first.request.baseRevision, artifacts: [] });
    await tick(); await tick();
    expect(calls('checkpoint_session_delta')).toHaveLength(2);
    await act(async () => { event({ type: 'done', data: { ...scope, cancelled: false } }); });
    expect(canonical.messages).toHaveLength(202);
    expect(canonical.messages.at(-1)).toMatchObject({ content: 'partial more', isStreaming: false });
    expect(journal).toBeUndefined();
    expect(hook.result.current.isStreaming).toBe(false);
    await tick(); expect(calls('checkpoint_session_delta')).toHaveLength(2);
  });
  it.each(['pin', 'rename', 'older'] as const)('folds the visible suffix before %s and continues journaling at the new revision', async (operation) => {
    const { hook, scope } = await started();
    act(() => event({ type: 'chunk', data: { ...scope, text: 'before' } }));
    await tick();
    act(() => event({ type: 'chunk', data: { ...scope, text: ' buffered' } }));
    await act(async () => {
      if (operation === 'pin') await hook.result.current.togglePinSession(scope.sessionId);
      else if (operation === 'rename') await hook.result.current.renameSession(scope.sessionId, 'Renamed');
      else await hook.result.current.loadOlderMessages();
    });
    expect(hook.result.current.lastError).toBeNull();
    expect(canonical.messages.at(-1)?.content).toBe('before buffered');
    if (operation === 'older') expect(hook.result.current.messages[0].id).toBe('m100');
    act(() => event({ type: 'chunk', data: { ...scope, text: ' after' } }));
    await tick();
    expect(journal?.messages[0].content).toBe('before buffered after');
    expect(hook.result.current.lastError).toBeNull();
    expect(calls('load_session')).toHaveLength(0);
  });

  it('retries disk errors without dropping buffered progress or artifact changes', async () => {
    const { hook, scope } = await started();
    failDelta = true;
    act(() => {
      event({ type: 'chunk', data: { ...scope, text: 'first' } });
      event({ type: 'artifact', data: { ...scope, artifact: { id: 'new', title: 'New', type: 'code', content: 'new data', timestamp: 5 } } });
    });
    await tick();
    expect(journal).toBeUndefined();
    expect(hook.result.current.lastError).toContain('temporary disk failure');
    act(() => event({ type: 'chunk', data: { ...scope, text: ' second' } }));
    await tick();
    expect(journal?.messages[0].content).toBe('first second');
    expect(journal?.artifacts[0].content).toBe('new data');
    await act(async () => { event({ type: 'done', data: { ...scope, cancelled: false } }); });
    expect(canonical.messages.at(-1)?.content).toBe('first second');
    expect(canonical.artifacts.some((a) => a.id === 'new')).toBe(true);
  });

  it('serializes pending journal IPC before final save and cancels rearming on done', async () => {
    const { scope } = await started();
    let release!: () => void;
    holdDelta = new Promise<void>((resolve) => { release = resolve; });
    act(() => event({ type: 'chunk', data: { ...scope, text: 'pending' } }));
    await tick();
    expect(calls('checkpoint_session_delta')).toHaveLength(1);
    const saves = calls('save_session_page').length;
    await act(async () => {
      event({ type: 'chunk', data: { ...scope, text: ' final' } });
      event({ type: 'done', data: { ...scope, cancelled: false } });
    });
    expect(calls('save_session_page')).toHaveLength(saves);
    await act(async () => { release(); await holdDelta; });
    expect(canonical.messages.at(-1)?.content).toBe('pending final');
    expect(journal).toBeUndefined();
    await tick(); expect(calls('checkpoint_session_delta')).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('does not acknowledge journal text as clean when navigating offscreen', async () => {
    const { hook, scope } = await started();
    act(() => event({ type: 'chunk', data: { ...scope, text: 'dirty journal' } }));
    await tick();
    const saves = calls('save_session_page').length;
    await act(async () => { hook.result.current.newSession(); });
    expect(calls('save_session_page')).toHaveLength(saves + 1);
    const viewed = hook.result.current.currentSessionId;
    const messages = hook.result.current.messages;
    act(() => event({ type: 'chunk', data: { ...scope, text: ' offscreen' } }));
    await tick();
    expect(journal?.messages[0].content).toBe('dirty journal offscreen');
    expect(hook.result.current.currentSessionId).toBe(viewed);
    expect(hook.result.current.messages).toBe(messages);
  });

  it('reads only a revision after a new-session full save before starting the provider', async () => {
    fresh = true;
    const { scope } = await started();
    const names = mocks.invoke.mock.calls.map(([name]) => name);
    expect(names.indexOf('save_session')).toBeLessThan(names.indexOf('load_session_page'));
    expect(names.indexOf('load_session_page')).toBeLessThan(names.indexOf('send_message'));
    expect(calls('load_session_page')[0]).toEqual({ sessionId: scope.sessionId, limit: 1 });
    act(() => event({ type: 'chunk', data: { ...scope, text: 'new session' } }));
    await tick();
    expect(journal?.messages[0].content).toBe('new session');
  });

  it('reconciles a new nonpaged session revision after pinning during a run', async () => {
    fresh = true;
    const { hook, scope } = await started();
    act(() => event({ type: 'chunk', data: { ...scope, text: 'new' } }));
    await tick();
    await act(async () => { await hook.result.current.togglePinSession(scope.sessionId); });
    act(() => event({ type: 'chunk', data: { ...scope, text: ' pinned' } }));
    await tick();
    expect(journal?.messages[0].content).toBe('new pinned');
    expect(hook.result.current.lastError).toBeNull();
  });

  it('does not rearm a pending checkpoint after hook teardown', async () => {
    const { hook, scope } = await started();
    let release!: () => void;
    holdDelta = new Promise<void>((resolve) => { release = resolve; });
    act(() => event({ type: 'chunk', data: { ...scope, text: 'pending' } }));
    await tick();
    hook.unmount();
    release(); await holdDelta;
    await tick();
    expect(calls('checkpoint_session_delta')).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(0);
  });
  it('waits for a pending delta before pinning and retains stream progress arriving during the mutation', async () => {
    const { hook, scope } = await started();
    let release!: () => void;
    holdDelta = new Promise<void>((resolve) => { release = resolve; });
    act(() => event({ type: 'chunk', data: { ...scope, text: 'journal' } }));
    await tick();
    let pin!: Promise<void>;
    act(() => { pin = hook.result.current.togglePinSession(scope.sessionId); });
    expect(calls('toggle_pin_session')).toHaveLength(0);
    act(() => event({ type: 'chunk', data: { ...scope, text: ' later' } }));
    await act(async () => { release(); await pin; });
    expect(canonical.messages.at(-1)?.content).toBe('journal');
    expect(canonical.session.pinned).toBe(true);
    await tick();
    expect(journal?.messages[0].content).toBe('journal later');
    expect(hook.result.current.lastError).toBeNull();
  });

  it('versions every artifact event while preserving existing immutable metadata', async () => {
    const { scope } = await started();
    const artifact = { id: 'old', title: 'Unchanged', type: 'code' as const, content: 'old artifact', timestamp: 1 };
    act(() => {
      event({ type: 'artifact', data: { ...scope, artifact } });
      event({ type: 'chunk', data: { ...scope, text: 'first' } });
    });
    await tick();
    expect(calls('checkpoint_session_delta')[0].request.artifacts).toEqual([{ ...artifact, streamRevision: 1 }]);
    act(() => event({ type: 'artifact', data: { ...scope, artifact: { ...artifact, title: 'Retitled', type: 'markdown', language: 'md', filePath: 'other.md', timestamp: 99, content: 'replacement' } } }));
    await tick();
    expect(calls('checkpoint_session_delta')[1].request.artifacts).toEqual([{ ...artifact, content: 'replacement', streamRevision: 2 }]);
    act(() => event({ type: 'artifact', data: { ...scope, artifact: { ...artifact, content: 'replacement' } } }));
    await tick();
    expect(calls('checkpoint_session_delta')[2].request.artifacts).toEqual([{ ...artifact, content: 'replacement', streamRevision: 3 }]);
  });

  it('saves trailing buffers after abort and rejects late events from the stopped run', async () => {
    const { hook, scope } = await started();
    act(() => event({ type: 'chunk', data: { ...scope, text: 'saved' } }));
    await tick();
    act(() => event({ type: 'chunk', data: { ...scope, text: ' trailing' } }));
    await act(async () => { await hook.result.current.stopAgent(); });
    expect(calls('abort_agent')).toEqual([{ runId: scope.runId }]);
    await act(async () => { event({ type: 'done', data: { ...scope, cancelled: true } }); });
    expect(canonical.messages.at(-1)).toMatchObject({ content: 'saved trailing', isStreaming: false, runStatus: 'cancelled' });
    act(() => event({ type: 'chunk', data: { ...scope, text: ' ignored' } }));
    await tick();
    expect(canonical.messages.at(-1)?.content).toBe('saved trailing');
    expect(calls('checkpoint_session_delta')).toHaveLength(1);
  });
  it('keeps a different viewed run alive after a late origin provider failure', async () => {
    const hook = renderHook(() => useAgent());
    await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
    const backend = mocks.invoke.getMockImplementation()!;
    let rejectStart!: (error: Error) => void;
    mocks.invoke.mockImplementation(async (command: string, args: any) => {
      if (command === 'send_message' && !rejectStart) return new Promise<void>((_, reject) => { rejectStart = reject; });
      return backend(command, args);
    });
    let sending!: Promise<boolean>;
    act(() => { sending = hook.result.current.sendMessage('origin A'); });
    await waitFor(() => expect(rejectStart).toBeTypeOf('function'));
    act(() => hook.result.current.newSession());
    await act(async () => { await hook.result.current.sendMessage('viewed B'); });
    const request = calls('send_message').at(-1).request;
    await act(async () => { rejectStart(new Error('origin failed')); await sending; });
    expect(hook.result.current.currentSessionId).toBe(request.sessionId);
    expect(hook.result.current.isStreaming).toBe(true);
    act(() => event({ type: 'chunk', data: { sessionId: request.sessionId, runId: request.runId, text: 'B alive' } }));
    await act(async () => { event({ type: 'done', data: { sessionId: request.sessionId, runId: request.runId, cancelled: false } }); });
    expect(hook.result.current.messages.at(-1)?.content).toBe('B alive');
  });
});
