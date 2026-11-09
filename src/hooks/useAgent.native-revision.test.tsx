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
afterEach(cleanup);

describe('useAgent native revision reconciliation', () => {
  it.each(['pin', 'rename'] as const)('uses a validated native cursor for the save following %s', async (kind) => {
    const hook = await mounted();
    const messages = hook.result.current.messages;
    await act(async () => {
      if (kind === 'pin') await hook.result.current.togglePinSession('one');
      else await hook.result.current.renameSession('one', 'Renamed');
    });
    expect(hook.result.current.messages).toBe(messages);
    await saveModel(hook);
    expect(accepted).toBe(1);
    expect(hook.result.current.lastError).toBeNull();
    expect(mocks.invoke).toHaveBeenCalledWith('refresh_session_window', expect.objectContaining({
      sessionId: 'one', allowMetadata: true, window: expect.objectContaining({ revision: 'r1' }),
      data: expect.objectContaining({ session: expect.objectContaining({ title: 'Original' }) }),
    }));
    expect(mocks.invoke.mock.calls.filter(([name]) => name === 'load_session_page')).toHaveLength(1);
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'load_session')).toBe(false);
  });

  it.each(['skill_staged', 'skill_updated'] as const)('reconciles a native %s event before review persistence and the next save', async (action) => {
    const hook = await mounted();
    const payload = review(action);
    canonical.messages[1].selfImprovement = payload;
    revision++;
    await act(async () => { mocks.listeners.get('agent:self-improvement')!({ payload }); });
    expect(hook.result.current.messages[1].selfImprovement?.action).toBe(action);
    expect(accepted).toBe(1);
    expect(mocks.invoke).toHaveBeenCalledWith('refresh_session_window', expect.objectContaining({
      sessionId: 'one', allowMetadata: false, reviewRunId: 'review-run',
      data: expect.objectContaining({ messages: expect.arrayContaining([expect.objectContaining({ id: 'a101', content: 'Answer' })]) }),
    }));
    const baseline = mocks.invoke.mock.calls.find(([name]) => name === 'refresh_session_window')![1].data;
    expect(baseline.messages[1].selfImprovement).toBeUndefined();
    await saveModel(hook);
    expect(accepted).toBe(2);
    expect(hook.result.current.lastError).toBeNull();
    expect(canonical.messages[1].selfImprovement?.action).toBe(action);
  });

  it('holds a queued save behind review reconciliation', async () => {
    const hook = await mounted();
    const gate = deferred<SessionWindow>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: Record<string, any>) => command === 'refresh_session_window' ? gate.promise : backend(command, args));
    const payload = review('skill_updated');
    canonical.messages[1].selfImprovement = payload;
    revision++;
    act(() => { mocks.listeners.get('agent:self-improvement')!({ payload }); hook.result.current.selectModel('p', 'queued'); });
    await act(async () => { await Promise.resolve(); });
    expect(mocks.invoke.mock.calls.some(([name]) => name === 'save_session_page')).toBe(false);
    await act(async () => { gate.resolve(currentWindow()); });
    expect(accepted).toBe(2);
    expect(hook.result.current.lastError).toBeNull();
  });

  it.each(['rename', 'pin', 'review'] as const)('does not upgrade over an external text edit during %s', async (kind) => {
    const hook = await mounted();
    // Same offset and first ID: the former rename boundary check accepted this.
    canonical.messages[1].content = 'Externally edited answer';
    revision++;
    await act(async () => {
      if (kind === 'rename') await hook.result.current.renameSession('one', 'Renamed');
      else if (kind === 'pin') await hook.result.current.togglePinSession('one');
      else {
        const payload = review('skill_updated');
        canonical.messages[1].selfImprovement = payload;
        revision++;
        mocks.listeners.get('agent:self-improvement')!({ payload });
      }
    });
    await saveModel(hook);
    expect(accepted).toBe(0);
    expect(canonical.messages[1].content).toBe('Externally edited answer');
    expect(hook.result.current.lastError).toMatch(/stale cursor revision/);
    expect(mocks.invoke).toHaveBeenCalledWith('refresh_session_window', expect.anything());
  });
});
