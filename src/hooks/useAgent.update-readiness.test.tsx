import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { pagingBackend } from '../test/sessionPaging';
import type { AgentSettings, SessionData } from '../types/agent';
import { useAgent } from './useAgent';

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));

const provider = { id: 'provider', name: 'Provider', kind: 'openai' as const, baseUrl: '', apiKey: '', model: 'model' };
const settings: AgentSettings = {
  providers: [provider], activeProviderId: provider.id, systemPrompt: '', temperature: 0.7,
  streamResponse: true, thinking: false, reasoningEffort: 'medium', favoriteModels: [],
  skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const initial: SessionData = {
  session: { id: 'origin', title: 'Origin', createdAt: 1, updatedAt: 1, messageCount: 0 },
  messages: [], artifacts: [],
};
const stored = new Map<string, SessionData>();
let saveGate: Promise<void> | undefined;
let settingsGate: Promise<void> | undefined;
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function mounted() {
  const hook = renderHook(useAgent);
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  return hook;
}
function saves() {
  return mocks.invoke.mock.calls.filter(([command]) => command === 'save_session' || command === 'save_session_page');
}
beforeEach(() => {
  stored.clear();
  stored.set(initial.session.id, structuredClone(initial));
  saveGate = undefined;
  settingsGate = undefined;
  mocks.invoke.mockReset();
  mocks.invoke.mockImplementation(pagingBackend(async (command: string, args?: any) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [...stored.values()].map((data) => data.session);
    if (command === 'load_session') return structuredClone(stored.get(args.sessionId)!);
    if (command === 'save_settings') return settingsGate;
    if (command === 'save_session') {
      await saveGate;
      stored.set(args.data.session.id, structuredClone(args.data));
    }
  }));
});
afterEach(cleanup);

describe('useAgent update readiness', () => {
  it('rejects an active run hidden behind an idle viewed session', async () => {
    const hook = await mounted();
    await act(async () => { expect(await hook.result.current.sendMessage('Keep working')).toBe(true); });
    act(() => { hook.result.current.newSession(); });
    expect(hook.result.current.isStreaming).toBe(false);
    expect(hook.result.current.runStatus).toBeNull();
    const count = saves().length;
    await act(async () => { await expect(hook.result.current.prepareForUpdate()).rejects.toThrow(/run|busy|idle/i); });
    expect(saves()).toHaveLength(count);
    expect(mocks.invoke.mock.calls.some(([command]) => /install.*update|update.*install/i.test(command))).toBe(false);
  });

  it('waits for the canonical dirty save before reporting readiness', async () => {
    const hook = await mounted();
    act(() => { hook.result.current.selectModel(provider.id, 'unsaved-model'); });
    expect(saves()).toHaveLength(0);
    const save = deferred<void>();
    saveGate = save.promise;
    let ready = false;
    let preparing!: Promise<void>;
    act(() => { preparing = hook.result.current.prepareForUpdate().then(() => { ready = true; }); });
    await waitFor(() => expect(saves()).toHaveLength(1));
    expect(saves()[0][0]).toBe('save_session_page');
    expect(stored.get('origin')!.modelOverride).toBeUndefined();
    expect(ready).toBe(false);
    await act(async () => { save.resolve(); await preparing; });
    expect(ready).toBe(true);
    expect(stored.get('origin')!.modelOverride?.model).toBe('unsaved-model');
    await act(async () => { await hook.result.current.prepareForUpdate(); });
    expect(saves()).toHaveLength(1);
  });

  it('rejects a failed canonical save and leaves the dirty session available for retry', async () => {
    const hook = await mounted();
    act(() => { hook.result.current.selectModel(provider.id, 'retry-model'); });
    const save = deferred<void>();
    saveGate = save.promise;
    const preparing = hook.result.current.prepareForUpdate().then(() => 'ready', () => 'rejected');
    await waitFor(() => expect(saves()).toHaveLength(1));
    await act(async () => { save.reject(new Error('disk full')); expect(await preparing).toBe('rejected'); });
    expect(stored.get('origin')!.modelOverride).toBeUndefined();
    expect(hook.result.current.lastError).toContain('disk full');
    saveGate = undefined;
    act(() => { hook.result.current.clearError(); });
    await act(async () => { await hook.result.current.prepareForUpdate(); });
    expect(stored.get('origin')!.modelOverride?.model).toBe('retry-model');
    expect(saves()).toHaveLength(2);
  });

  it('rejects navigation during a save even when the viewed session returns to its origin', async () => {
    const hook = await mounted();
    act(() => { hook.result.current.selectModel(provider.id, 'navigation-model'); });
    const save = deferred<void>();
    saveGate = save.promise;
    const preparing = hook.result.current.prepareForUpdate().then(() => 'ready', () => 'rejected');
    await waitFor(() => expect(saves()).toHaveLength(1));
    act(() => { hook.result.current.newSession(); });
    await act(async () => { await hook.result.current.switchSession('origin'); });
    expect(hook.result.current.currentSessionId).toBe('origin');
    await act(async () => { save.resolve(); expect(await preparing).toBe('rejected'); });
  });

  it('rejects when the existing settings save fails instead of trusting the caught queue tail', async () => {
    const hook = await mounted();
    const save = deferred<void>();
    settingsGate = save.promise;
    let updating!: Promise<unknown>;
    act(() => { updating = hook.result.current.updateSettings({ ...settings, temperature: 0.2 }).catch((error) => error); });
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith('save_settings', expect.anything()));
    const preparing = hook.result.current.prepareForUpdate().then(() => 'ready', () => 'rejected');
    await act(async () => { save.reject(new Error('settings disk full')); await updating; });
    expect(await preparing).toBe('rejected');
    expect(saves()).toHaveLength(0);
  });
});
