import { pagingBackend } from '../test/sessionPaging';
﻿import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent, AgentSettings, SessionData } from '../types/agent';
import { useAgent } from './useAgent';

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

const provider = { id: 'provider', name: 'Provider', kind: 'openai' as const, baseUrl: 'http://localhost:1234/v1', apiKey: '', model: 'model' };
const settings: AgentSettings = {
  providers: [provider], activeProviderId: provider.id, systemPrompt: '', temperature: 0.7,
  streamResponse: true, thinking: false, reasoningEffort: 'medium', favoriteModels: [],
  skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const initial: SessionData = {
  session: { id: 'council-session', title: 'Council', createdAt: 1, updatedAt: 1, messageCount: 0 },
  messages: [], artifacts: [],
};
function event(payload: AgentEvent) {
  expect(mocks.listeners.has('agent-event')).toBe(true);
  mocks.listeners.get('agent-event')!({ payload });
}
const saves = () => mocks.invoke.mock.calls.filter(([name]) => name === 'save_session_page');
async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  return hook;
}

beforeEach(() => {
  mocks.listeners.clear();
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(pagingBackend(async (command: string) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [initial.session];
    if (command === 'load_session') return structuredClone(initial);
    return undefined;
  }));
});

describe('useAgent Council winner completion barrier', () => {
  it.each(['A', 'B'] as const)('ignores early winner %s and copies the full final answer only after parent done', async (winner) => {
    const hook = await mounted();
    let frame!: FrameRequestCallback;
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => { frame = callback; return 123; });
    await act(async () => {
      await hook.result.current.sendMessage('Compare answers', undefined, {
        council: true, candidateA: { provider }, candidateB: { provider },
      });
    });
    const message = hook.result.current.messages.at(-1)!;
    const scope = { sessionId: initial.session.id, runId: message.runId! };
    act(() => {
      event({ type: 'council_chunk', data: { ...scope, candidate: winner, kind: 'content', text: 'Partial' } });
      frame(0);
    });
    expect(hook.result.current.messages.at(-1)?.councilTurn?.[winner === 'A' ? 'candidateA' : 'candidateB'].content).toBe('Partial');

    async function expectSelectionIgnored() {
      const before = hook.result.current.messages;
      const saveCount = saves().length;
      await act(async () => { hook.result.current.selectCouncilWinner(message.id, winner); });
      expect(hook.result.current.messages).toBe(before);
      expect(hook.result.current.messages.at(-1)?.content).toBe('');
      expect(hook.result.current.messages.at(-1)?.councilTurn?.chosenWinner).toBeUndefined();
      expect(saves()).toHaveLength(saveCount);
    }
    await expectSelectionIgnored();
    act(() => event({ type: 'council_done', data: { ...scope, candidate: winner, durationMs: 10 } }));
    await expectSelectionIgnored();
    act(() => event({ type: 'council_done', data: { ...scope, candidate: winner === 'A' ? 'B' : 'A', durationMs: 20 } }));
    // Both candidates can finish before the parent run completes.
    expect(hook.result.current.isStreaming).toBe(true);
    await expectSelectionIgnored();

    await act(async () => {
      // Keep the trailing chunk buffered: no animation frame runs before selection.
      event({ type: 'council_chunk', data: { ...scope, candidate: winner, kind: 'content', text: ' final answer' } });
      event({ type: 'done', data: { ...scope, cancelled: false, successful: true } });
      hook.result.current.selectCouncilWinner(message.id, winner);
    });
    expect(hook.result.current.isStreaming).toBe(false);
    expect(hook.result.current.messages.at(-1)).toMatchObject({
      content: 'Partial final answer', isStreaming: false, runStatus: 'completed', councilTurn: { chosenWinner: winner },
    });
    expect((saves().at(-1)![1].data as SessionData).messages.at(-1)?.content).toBe('Partial final answer');
    await act(async () => { await hook.result.current.sendMessage('Follow up'); });
    const request = mocks.invoke.mock.calls.find(([name]) => name === 'send_message')![1].request;
    expect(request.messages).toContainEqual(expect.objectContaining({ role: 'assistant', content: 'Partial final answer' }));
  });

  it.each([[true, false], [false, true]])('rejects a streaming candidate on an otherwise finished restored turn (%s, %s)', async (streamingA, streamingB) => {
    const restored = structuredClone(initial);
    restored.messages = [{
      id: 'finished', role: 'assistant', content: '', timestamp: 1, runId: 'finished-run', runStatus: 'completed',
      councilTurn: {
        candidateA: { providerId: provider.id, model: 'A', content: 'A', isStreaming: streamingA },
        candidateB: { providerId: provider.id, model: 'B', content: 'B', isStreaming: streamingB },
      },
    }];
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(pagingBackend((command: string, args: unknown) => command === 'load_session' ? Promise.resolve(restored) : backend(command, args)));
    const hook = await mounted();
    const before = hook.result.current.messages;
    const saveCount = saves().length;
    await act(async () => {
      hook.result.current.selectCouncilWinner('finished', 'A');
      hook.result.current.selectCouncilWinner('finished', 'B');
    });
    expect(hook.result.current.messages).toBe(before);
    expect(saves()).toHaveLength(saveCount);
  });
});
