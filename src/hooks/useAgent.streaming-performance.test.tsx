import { pagingBackend } from '../test/sessionPaging';
import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent, AgentSettings, SessionData } from '../types/agent';
import { useAgent } from './useAgent';

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  inHook: false,
  stateWrites: vi.fn(),
  setters: new WeakMap<(next: unknown) => void, (next: unknown) => void>(),
}));
vi.mock('react', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react')>();
  return {
    ...actual,
    useState: (initial?: unknown) => {
      const [value, setter] = actual.useState(initial);
      // Hitung kerja hook saja, bukan state internal renderHook atau jumlah render.
      if (!mocks.inHook) return [value, setter];
      let counted = mocks.setters.get(setter);
      if (!counted) {
        counted = (next: unknown) => { mocks.stateWrites(); setter(next); };
        mocks.setters.set(setter, counted);
      }
      return [value, counted];
    },
  };
});
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    mocks.listeners.set(name, handler);
    return () => mocks.listeners.delete(name);
  }),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));

const provider = { id: 'provider', name: 'Provider', kind: 'openai' as const, baseUrl: 'http://localhost:1234/v1', apiKey: '', model: 'model' };
const settings: AgentSettings = {
  providers: [provider], activeProviderId: provider.id, systemPrompt: '', temperature: 0.7,
  streamResponse: true, thinking: false, reasoningEffort: 'medium', favoriteModels: [],
  skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const initial: SessionData = {
  session: { id: 'stream-session', title: 'Stream', createdAt: 1, updatedAt: 1, messageCount: 0 },
  messages: [], artifacts: [],
};
function event(payload: AgentEvent) {
  expect(mocks.listeners.has('agent-event')).toBe(true);
  mocks.listeners.get('agent-event')!({ payload });
}
const snapshots = (command: string) => mocks.invoke.mock.calls
  .filter(([name]) => name === command).map(([, args]) => args.data as SessionData);
const deltas = () => mocks.invoke.mock.calls.filter(([name]) => name === 'checkpoint_session_delta').map(([, args]) => args.request);
async function started(council = false) {
  const hook = renderHook(() => {
    mocks.inHook = true;
    try { return useAgent(); } finally { mocks.inHook = false; }
  });
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  await act(async () => {
    expect(await hook.result.current.sendMessage('Stream', undefined, council ? {
      council: true, candidateA: { provider }, candidateB: { provider },
    } : undefined)).toBe(true);
  });
  const scope = { sessionId: initial.session.id, runId: hook.result.current.messages.at(-1)!.runId! };
  const frames: FrameRequestCallback[] = [];
  const raf = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
    frames.push(callback);
    return frames.length;
  });
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => undefined);
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
  mocks.stateWrites.mockClear();
  return { hook, scope, raf, frames };
}
beforeEach(() => {
  mocks.listeners.clear();
  mocks.stateWrites.mockClear();
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(pagingBackend(async (command: string) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [initial.session];
    if (command === 'load_session') return structuredClone(initial);
    return undefined;
  }));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('useAgent bounded streaming publication', () => {
  it.each(['chunk', 'reasoning', 'terminal_output', 'A content', 'A reasoning', 'B content', 'B reasoning'] as const)(
    'buffers 200 %s events with zero setters and one RAF, then publishes once', async (kind) => {
      const { hook, scope, raf, frames } = await started(kind.includes(' '));
      if (kind === 'terminal_output') {
        act(() => event({ type: 'artifact', data: { ...scope, artifact: {
          id: 'terminal', type: 'terminal', title: 'Terminal', content: '', timestamp: 1,
        } } }));
        mocks.stateWrites.mockClear();
      }
      const messages = hook.result.current.messages;
      const artifacts = hook.result.current.artifacts;
      act(() => {
        for (let i = 0; i < 200; i++) {
          if (kind === 'chunk' || kind === 'reasoning') event({ type: kind, data: { ...scope, text: 'x' } });
          else if (kind === 'terminal_output') event({ type: kind, data: { ...scope, artifactId: 'terminal', delta: 'x', stream: 'stdout' } });
          else event({ type: 'council_chunk', data: {
            ...scope, candidate: kind.startsWith('A') ? 'A' : 'B', kind: kind.endsWith('reasoning') ? 'reasoning' : 'content', text: 'x',
          } });
        }
      });
      expect(mocks.stateWrites).toHaveBeenCalledTimes(0);
      expect(raf).toHaveBeenCalledTimes(1);
      expect(vi.getTimerCount()).toBe(1);
      expect(hook.result.current.messages).toBe(messages);
      expect(hook.result.current.artifacts).toBe(artifacts);
      act(() => frames[0](0));
      expect(mocks.stateWrites).toHaveBeenCalledTimes(6);
      const message = hook.result.current.messages.at(-1)!;
      const buffered = kind === 'chunk' ? message.content : kind === 'reasoning' ? message.reasoning
        : kind === 'terminal_output' ? hook.result.current.artifacts[0].content
          : message.councilTurn![kind.startsWith('A') ? 'candidateA' : 'candidateB'][kind.endsWith('reasoning') ? 'reasoning' : 'content'];
      expect(buffered).toBe('x'.repeat(200));
    },
  );

  it.each(['chunk', 'reasoning'] as const)('clears a notice immediately on %s without publishing subsequent buffered events', async (type) => {
    const { hook, scope, raf, frames } = await started();
    act(() => event({ type: 'notice', data: { ...scope, message: 'Retrying', attempt: 1, maxAttempts: 3 } }));
    expect(hook.result.current.messages.at(-1)?.notice?.message).toBe('Retrying');
    mocks.stateWrites.mockClear();
    act(() => event({ type, data: { ...scope, text: 'first' } }));
    expect(hook.result.current.messages.at(-1)?.notice).toBeNull();
    expect(hook.result.current.messages.at(-1)?.content).toBe('');
    expect(hook.result.current.messages.at(-1)?.reasoning || '').toBe('');
    expect(mocks.stateWrites).toHaveBeenCalledTimes(6);
    mocks.stateWrites.mockClear();
    act(() => event({ type, data: { ...scope, text: ' last' } }));
    expect(mocks.stateWrites).toHaveBeenCalledTimes(0);
    expect(raf).toHaveBeenCalledTimes(1);
    act(() => frames[0](0));
    expect(hook.result.current.messages.at(-1)?.[type === 'chunk' ? 'content' : 'reasoning']).toBe('first last');
  });

  it('checkpoints every buffer without a frame and includes the trailing buffers in the final save', async () => {
    const { hook, scope, raf } = await started(true);
    act(() => event({ type: 'artifact', data: { ...scope, artifact: {
      id: 'terminal', type: 'terminal', title: 'Terminal', content: '', timestamp: 1,
    } } }));
    const buffer = (text: string) => {
      event({ type: 'chunk', data: { ...scope, text } });
      event({ type: 'reasoning', data: { ...scope, text } });
      event({ type: 'terminal_output', data: { ...scope, artifactId: 'terminal', delta: text, stream: 'stdout' } });
      event({ type: 'council_chunk', data: { ...scope, candidate: 'A', kind: 'content', text } });
      event({ type: 'council_chunk', data: { ...scope, candidate: 'B', kind: 'reasoning', text } });
    };
    mocks.stateWrites.mockClear();
    act(() => buffer('partial'));
    expect(mocks.stateWrites).toHaveBeenCalledTimes(0);
    expect(raf).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    const checkpoints = deltas();
    expect(checkpoints).toHaveLength(1);
    expect(checkpoints[0].messages).toHaveLength(1);
    expect(checkpoints[0].messages[0]).toMatchObject({ role: 'assistant', runId: scope.runId });
    expect(checkpoints[0].artifacts.map((artifact: { id: string }) => artifact.id)).toEqual(['terminal']);
    expect(checkpoints[0]).not.toHaveProperty('historyWindow');
    expect(checkpoints[0]).not.toHaveProperty('session');
    expect(snapshots('checkpoint_session_page')).toHaveLength(0);
    const expectBuffers = (data: Pick<SessionData, 'messages' | 'artifacts'>, text: string) => {
      expect(data.messages.at(-1)).toMatchObject({
        content: text, reasoning: text, segments: [{ text }],
        councilTurn: { candidateA: { content: text }, candidateB: { reasoning: text } },
      });
      expect(data.artifacts[0].content).toBe(text);
    };
    expectBuffers(checkpoints[0], 'partial');
    expect(checkpoints[0].messages.at(-1)?.isStreaming).toBe(true);
    expect(mocks.stateWrites).toHaveBeenCalledTimes(6);
    await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
    expect(deltas()).toHaveLength(1);
    await act(async () => {
      buffer(' last');
      event({ type: 'done', data: { ...scope, cancelled: false, successful: true } });
    });
    const saved = snapshots('save_session_page').at(-1)!;
    expectBuffers(saved, 'partial last');
    expect(saved.messages.at(-1)).toMatchObject({ isStreaming: false, runStatus: 'completed' });
    expect(hook.result.current.messages.at(-1)?.content).toBe('partial last');
    expect(hook.result.current.isStreaming).toBe(false);
    // Snapshot lama tidak boleh ikut berubah ketika buffer berikutnya dikuras.
    expectBuffers(checkpoints[0], 'partial');
    await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
    expect(deltas()).toHaveLength(1);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('buffers and checkpoints an offscreen origin without publishing into the viewed session', async () => {
    const { hook, scope, raf } = await started();
    await act(async () => { hook.result.current.newSession(); });
    const viewed = hook.result.current.currentSessionId;
    const messages = hook.result.current.messages;
    mocks.stateWrites.mockClear();
    act(() => {
      for (let i = 0; i < 200; i++) event({ type: 'chunk', data: { ...scope, text: 'x' } });
    });
    expect(raf).toHaveBeenCalledTimes(1);
    expect(mocks.stateWrites).toHaveBeenCalledTimes(0);
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(mocks.stateWrites).toHaveBeenCalledTimes(0);
    expect(deltas()).toHaveLength(1);
    expect(deltas()[0].messages).toHaveLength(1);
    expect(deltas()[0].messages[0]).toMatchObject({ runId: scope.runId, content: 'x'.repeat(200) });
    expect(deltas()[0].artifacts).toEqual([]);
    expect(deltas()[0]).not.toHaveProperty('historyWindow');
    expect(hook.result.current.currentSessionId).toBe(viewed);
    expect(hook.result.current.messages).toBe(messages);
    await act(async () => {
      event({ type: 'chunk', data: { ...scope, text: ' last' } });
      event({ type: 'done', data: { ...scope, cancelled: false } });
    });
    await act(async () => { await hook.result.current.switchSession(scope.sessionId); });
    expect(hook.result.current.messages.at(-1)).toMatchObject({ content: `${'x'.repeat(200)} last`, runStatus: 'completed' });
    expect(hook.result.current.isStreaming).toBe(false);
    expect(snapshots('save_session_page').at(-1)?.messages.at(-1)?.content).toBe(`${'x'.repeat(200)} last`);
    await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
    expect(deltas()).toHaveLength(1);
    expect(snapshots('checkpoint_session_page')).toHaveLength(0);
  });

  it('flushes text and reasoning at tool barriers and preserves segment order through done', async () => {
    const { hook, scope } = await started();
    act(() => {
      event({ type: 'chunk', data: { ...scope, text: 'Before tool' } });
      event({ type: 'reasoning', data: { ...scope, text: 'Thinking' } });
      event({ type: 'tool_update', data: { ...scope, tool: {
        id: 'tool', name: 'read_file', args: {}, status: 'running',
      } } });
    });
    expect(hook.result.current.messages.at(-1)).toMatchObject({
      preamble: 'Before tool', content: '', reasoning: 'Thinking',
      segments: [{ text: 'Before tool', toolCalls: [{ id: 'tool' }] }],
    });
    await act(async () => {
      event({ type: 'chunk', data: { ...scope, text: 'After tool' } });
      event({ type: 'done', data: { ...scope, cancelled: false } });
    });
    expect(hook.result.current.messages.at(-1)).toMatchObject({
      preamble: 'Before tool', content: 'After tool', reasoning: 'Thinking',
      segments: [{ text: 'Before tool', toolCalls: [{ id: 'tool' }] }, { text: 'After tool', toolCalls: [] }],
    });
    // Done may schedule index maintenance, never another run checkpoint.
    const checkpointCount = deltas().length;
    const warmCount = mocks.invoke.mock.calls.filter(([name]) => name === 'warm_session_index').length;
    expect(vi.getTimerCount()).toBe(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'warm_session_index')).toHaveLength(warmCount + 1);
    expect(deltas()).toHaveLength(checkpointCount);
    expect(snapshots('checkpoint_session_page')).toHaveLength(0);
    expect(vi.getTimerCount()).toBe(0);
  });
});
