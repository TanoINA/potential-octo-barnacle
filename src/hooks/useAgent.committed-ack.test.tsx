import { createHash } from 'node:crypto';
import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSettings, SessionData, SessionPage, SessionWindow } from '../types/agent';
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
type Proof = { version: number; sessionId: string; canonicalSha256: string; offset: number; total: number; firstMessageId: string | null };
let canonical: SessionData;
let revision: number;
let failNextSave: boolean;
let lastProof: Proof | undefined;
const saves = () => mocks.invoke.mock.calls.filter(([name]) => name === 'save_session_page' || name === 'save_session');
const recoveries = () => mocks.invoke.mock.calls.filter(([name]) => name === 'recover_save_ack');
const reads = () => mocks.invoke.mock.calls.filter(([name]) => name === 'load_session_page' || name === 'load_session');
const hash = () => createHash('sha256').update(JSON.stringify(canonical)).digest('hex');
function windowAt(offset: number): SessionWindow {
  return { offset, total: canonical.messages.length, revision: `r${revision}`, firstMessageId: canonical.messages[offset]?.id ?? null };
}
function exactPage(offset: number): SessionPage {
  return { data: structuredClone({ ...canonical, messages: canonical.messages.slice(offset) }), window: windowAt(offset) };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  expect(hook.result.current.currentSessionId).toBe('one');
  return hook;
}
async function model(hook: Awaited<ReturnType<typeof mounted>>, name: string) {
  await act(async () => { hook.result.current.selectModel('p', name); });
}
function stream(hook: Awaited<ReturnType<typeof mounted>>, text: string) {
  const message = hook.result.current.messages.at(-1)!;
  let frame!: FrameRequestCallback;
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => { frame = callback; return 1; });
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation(() => undefined);
  act(() => { mocks.listeners.get('agent-event')!({ payload: { type: 'chunk', data: { sessionId: 'one', runId: message.runId, text } } }); });
  act(() => { frame(0); });
}

beforeEach(() => {
  canonical = {
    session: { id: 'one', title: 'Original', createdAt: 1, updatedAt: 2, messageCount: 102 },
    messages: Array.from({ length: 102 }, (_, i) => ({ id: `m${i}`, role: 'user', content: `Original ${i}`, timestamp: i, isStreaming: false })),
    artifacts: [{ id: 'unloaded', type: 'code', title: 'Native artifact', content: 'Preserve me', timestamp: 1 }],
    modelOverride: null,
  };
  canonical.messages[101] = { ...canonical.messages[101], role: 'assistant', segments: [{ id: 'seg_0', text: 'Original 101', toolCalls: [] }],
    providerMessages: [{ role: 'assistant', opaque: 'native replay bytes' }] };
  revision = 1;
  failNextSave = false;
  lastProof = undefined;
  mocks.listeners.clear();
  mocks.invoke.mockReset();
  // Canonical bytes, disposable projection revision, and acknowledgements have
  // independent lifetimes. A rejected save really commits before it throws.
  mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [structuredClone(canonical.session)];
    if (command === 'load_session') throw new Error('Recovery must not full-load history');
    if (command === 'load_session_page') {
      if (args.revision && args.revision !== `r${revision}`) throw new Error('stale cursor revision');
      const end = args.before ?? canonical.messages.length;
      const offset = Math.max(0, end - args.limit);
      const page = exactPage(offset);
      page.data.messages = page.data.messages.slice(0, end - offset).map(({ providerMessages, ...message }) =>
        providerMessages ? { ...message, hasProviderMessages: true } : message);
      page.data.artifacts = [];
      return page;
    }
    if (command === 'save_session_page' || command === 'save_session') {
      const offset = args.window?.offset ?? 0;
      if (args.window && args.window.revision !== `r${revision}`) throw new Error('stale cursor revision');
      const suffix = canonical.messages.slice(offset);
      if (args.window && (args.window.firstMessageId !== suffix[0]?.id || args.data.messages.length < suffix.length
        || suffix.some((stored, index) => stored.id !== args.data.messages[index]?.id))) throw new Error('canonical tail boundary changed');
      const messages = args.data.messages.map((message: SessionData['messages'][number]) => {
        const stored = canonical.messages.find((item) => item.id === message.id);
        const { hasProviderMessages: _providerHint, hasImageInputs: _imageHint, ...snapshot } = message;
        return { ...snapshot, ...(stored?.providerMessages && !message.providerMessages ? { providerMessages: stored.providerMessages } : {}),
          ...(stored?.imageInputs && !message.imageInputs ? { imageInputs: stored.imageInputs } : {}) };
      });
      canonical = structuredClone({ ...args.data, messages: [...canonical.messages.slice(0, offset), ...messages],
        artifacts: [...canonical.artifacts.filter((old) => !args.data.artifacts.some((item: { id: string }) => item.id === old.id)), ...args.data.artifacts] });
      revision++;
      if (failNextSave) {
        failNextSave = false;
        lastProof = { version: 1, sessionId: canonical.session.id, canonicalSha256: hash(), offset,
          total: canonical.messages.length, firstMessageId: canonical.messages[offset]?.id ?? null };
        throw '[canonical-committed]' + JSON.stringify({ proof: lastProof, cause: 'SQLite projection acknowledgement failed' });
      }
      // The native full-save contract returns void, never a cursor/page.
      return args.window ? windowAt(offset) : undefined;
    }
    if (command === 'recover_save_ack') {
      expect(args.sessionId).toBe(canonical.session.id);
      expect(args.proof).toEqual(lastProof);
      if (args.proof.canonicalSha256 !== hash()) throw new Error('Canonical changed after commit; recovery conflict, snapshot preserved');
      return exactPage(args.proof.offset);
    }
    if (command === 'toggle_pin_session') {
      canonical.session = { ...canonical.session, pinned: !canonical.session.pinned, pinnedAt: 42 };
      revision++;
      return structuredClone(canonical.session);
    }
    if (command === 'refresh_session_window') {
      expect(args.data.historyWindow).toBeUndefined();
      const comparable = (message: SessionData['messages'][number]) => {
        const stored = canonical.messages.find((item) => item.id === message.id);
        const { hasProviderMessages: _providerHint, hasImageInputs: _imageHint, ...snapshot } = message;
        return { ...snapshot, ...(stored?.providerMessages && !message.providerMessages ? { providerMessages: stored.providerMessages } : {}),
          ...(stored?.imageInputs && !message.imageInputs ? { imageInputs: stored.imageInputs } : {}) };
      };
      expect(args.data.messages.map(comparable)).toEqual(canonical.messages.slice(args.window.offset).map(comparable));
      for (const artifact of args.data.artifacts) expect(artifact).toEqual(canonical.artifacts.find((item) => item.id === artifact.id));
      expect(args.data.modelOverride).toEqual(canonical.modelOverride);
      return windowAt(args.window.offset);
    }
    return undefined;
  });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

describe('canonical-committed save acknowledgement recovery', () => {
  it('recovers a void full-save acknowledgement without a one-row revision read or a second write', async () => {
    const hook = await mounted();
    act(() => { hook.result.current.newSession(); });
    const sessionId = hook.result.current.currentSessionId;
    // Create-only full save has no canonical prefix to merge.
    canonical = { session: { id: sessionId, title: 'New Chat', createdAt: 1, updatedAt: 1, messageCount: 0 }, messages: [], artifacts: [] };
    const readCount = reads().length;
    const saveCount = saves().length;
    failNextSave = true;
    await act(async () => { await hook.result.current.sendMessage('Start from full snapshot'); });
    expect(recoveries()).toHaveLength(1);
    expect(lastProof).toMatchObject({ sessionId, offset: 0, total: 2 });
    expect(saves()).toHaveLength(saveCount + 1);
    expect(saves().at(-1)![0]).toBe('save_session');
    expect(reads()).toHaveLength(readCount);
    expect(hook.result.current.lastError).toBeNull();
    expect(hook.result.current.messages.at(-1)!.content).toBe('');
    expect(hook.result.current.isStreaming).toBe(true);
    expect(mocks.invoke).toHaveBeenCalledWith('send_message', expect.objectContaining({ request: expect.objectContaining({ sessionId }) }));
    await model(hook, 'ordinary-void-save');
    expect(saves()).toHaveLength(saveCount + 2);
    expect(canonical.modelOverride?.model).toBe('ordinary-void-save');
    expect(hook.result.current.lastError).toBeNull();
  });
  it.each([
    ['invalid JSON', () => 'not json'],
    ['null receipt', () => 'null'],
    ['null proof', (receipt: any) => ({ ...receipt, proof: null })],
    ['array proof', (receipt: any) => ({ ...receipt, proof: [] })],
    ['missing cause', ({ cause: _cause, ...receipt }: any) => receipt],
    ['non-string cause', (receipt: any) => ({ ...receipt, cause: 1 })],
    ['extra receipt field', (receipt: any) => ({ ...receipt, extra: true })],
    ['extra proof field', (receipt: any) => ({ ...receipt, proof: { ...receipt.proof, extra: true } })],
    ...[
      ['version', 2], ['version', '1'], ['sessionId', 'foreign'], ['sessionId', ''],
      ['offset', -1], ['offset', 52.5], ['offset', 51], ['offset', '52'],
      ['total', 101], ['total', 103], ['total', Number.MAX_SAFE_INTEGER + 1],
      ['firstMessageId', 'm53'], ['firstMessageId', null], ['firstMessageId', 52],
      ['canonicalSha256', 'x'.repeat(64)], ['canonicalSha256', 'a'.repeat(63)], ['canonicalSha256', null],
    ].map(([key, value]): [string, (receipt: any) => any] => [`${key}=${value}`, (receipt: any) => ({ ...receipt, proof: { ...receipt.proof, [key as string]: value } })]),
    ['missing firstMessageId', (receipt: any) => {
      const { firstMessageId: _id, ...proof } = receipt.proof;
      return { ...receipt, proof };
    }],
  ] as const)('rejects %s before recovery IPC without silently rebasing', async (_name, corrupt) => {
    const hook = await mounted();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
      try { return await backend(command, args); }
      catch (error) {
        if (command !== 'save_session_page' || !String(error).startsWith('[canonical-committed]')) throw error;
        const broken = corrupt(JSON.parse(String(error).slice('[canonical-committed]'.length)));
        throw '[canonical-committed]' + (typeof broken === 'string' ? broken : JSON.stringify(broken));
      }
    });
    failNextSave = true;
    await model(hook, 'committed-but-untrusted');
    expect(canonical.modelOverride?.model).toBe('committed-but-untrusted');
    expect(recoveries()).toHaveLength(0);
    expect(hook.result.current.lastError).toMatch(/Could not save session/);
    expect(reads()).toHaveLength(1);
    mocks.invoke.mockImplementation(backend);
    await model(hook, 'still-local');
    expect(saves().at(-1)![1].window.revision).toBe('r1');
    expect(hook.result.current.lastError).toMatch(/stale cursor revision/);
    expect(canonical.modelOverride?.model).toBe('committed-but-untrusted');
  });
  it('keeps newer streamed text dirty when a delayed recovery acknowledges an older snapshot', async () => {
    const hook = await mounted();
    await act(async () => { await hook.result.current.sendMessage('Continue'); });
    stream(hook, 'committed seed');
    const backend = mocks.invoke.getMockImplementation()!;
    const recovered = deferred<SessionPage>();
    const release = deferred<SessionPage>();
    mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
      const result = await backend(command, args);
      if (command === 'recover_save_ack') {
        recovered.resolve(result);
        return release.promise;
      }
      return result;
    });
    const saveCount = saves().length;
    const readCount = reads().length;
    failNextSave = true;
    let page!: SessionPage;
    await act(async () => {
      hook.result.current.selectModel('p', 'recovering');
      page = await recovered.promise;
    });
    expect(lastProof).toMatchObject({ sessionId: 'one', offset: 52, total: 104, firstMessageId: 'm52' });
    const canonicalBytes = JSON.stringify(canonical);
    stream(hook, ' newer');
    const newerMessages = hook.result.current.messages;
    expect(newerMessages.at(-1)!.content).toBe('committed seed newer');
    await act(async () => { release.resolve(page); });
    expect(hook.result.current.messages).toBe(newerMessages); // Cursor-only ack cannot publish the old content.
    expect(hook.result.current.isStreaming).toBe(true);
    expect(hook.result.current.lastError).toBeNull();
    expect(JSON.stringify(canonical)).toBe(canonicalBytes);
    expect(saves()).toHaveLength(saveCount + 1);
    expect(recoveries()).toHaveLength(1);
    expect(reads()).toHaveLength(readCount);

    // An unloaded native-review notification reconciles committedData without
    // first saving the newer UI, exposing the exact recovered baseline.
    await act(async () => { mocks.listeners.get('agent:self-improvement')!({ payload: {
      session_id: 'one', run_id: 'unloaded-review', action: 'skill_updated', changes: [],
    } }); });
    const refresh = mocks.invoke.mock.calls.find(([name]) => name === 'refresh_session_window')![1];
    expect(refresh.window).toEqual(page.window);
    expect(refresh.data.messages).toEqual(page.data.messages);
    expect(refresh.data.messages.at(-1).content).toBe('committed seed');
    expect(refresh.data.messages.find((message: { id: string }) => message.id === 'm101').providerMessages)
      .toEqual([{ role: 'assistant', opaque: 'native replay bytes' }]);
    expect(refresh.data.artifacts).toEqual(page.data.artifacts);
    expect(saves()).toHaveLength(saveCount + 1);

    await act(async () => { await hook.result.current.togglePinSession('one'); });
    const flushed = saves()[saveCount + 1];
    expect(flushed[1].data.messages.at(-1).content).toBe('committed seed newer');
    expect(flushed[1].window).toMatchObject({ offset: 52, revision: page.window.revision, firstMessageId: 'm52' });
    expect(mocks.invoke.mock.calls.indexOf(flushed)).toBeLessThan(mocks.invoke.mock.calls.findIndex(([name]) => name === 'toggle_pin_session'));
    await model(hook, 'after-delayed-recovery');
    expect(canonical.messages.at(-1)!.content).toBe('committed seed newer');
    expect(canonical.messages[101].providerMessages).toEqual([{ role: 'assistant', opaque: 'native replay bytes' }]);
    expect(canonical.artifacts[0].id).toBe('unloaded');
    expect(hook.result.current.lastError).toBeNull();
  });

  it('retains queued snapshot offsets when an older page arrives before the failed save receipt', async () => {
    const hook = await mounted();
    const backend = mocks.invoke.getMockImplementation()!;
    const olderReady = deferred<SessionPage>();
    const olderRelease = deferred<SessionPage>();
    const committed = deferred<unknown>();
    const receiptRelease = deferred<void>();
    mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
      try {
        const result = await backend(command, args);
        if (command === 'load_session_page' && args.before !== undefined) {
          olderReady.resolve(result);
          return olderRelease.promise;
        }
        return result;
      } catch (error) {
        if (command === 'save_session_page' && String(error).startsWith('[canonical-committed]')) {
          committed.resolve(error);
          await receiptRelease.promise;
        }
        throw error;
      }
    });
    const prefix = structuredClone(canonical.messages.slice(0, 52));
    let loading!: Promise<void>;
    let older!: SessionPage;
    await act(async () => {
      loading = hook.result.current.loadOlderMessages();
      older = await olderReady.promise;
    });
    expect(older.window).toMatchObject({ offset: 2, revision: 'r1', firstMessageId: 'm2' });
    failNextSave = true;
    await act(async () => {
      hook.result.current.selectModel('p', 'queued-first');
      await committed.promise;
    });
    act(() => { hook.result.current.selectModel('p', 'queued-second'); });
    expect(saves()).toHaveLength(1); // Second snapshot is queued, not yet submitted.
    expect(recoveries()).toHaveLength(0);
    await act(async () => { olderRelease.resolve(older); await loading; });
    const expandedMessages = hook.result.current.messages;
    expect(expandedMessages).toHaveLength(100);
    expect(expandedMessages[0].id).toBe('m2');
    expect(hook.result.current.modelOverride?.model).toBe('queued-second');
    await act(async () => { receiptRelease.resolve(); });
    expect(recoveries()).toHaveLength(1);
    expect(recoveries()[0][1].proof).toMatchObject({ sessionId: 'one', offset: 52, total: 102, firstMessageId: 'm52' });
    expect(saves()).toHaveLength(2);
    for (const [index, [, args]] of saves().entries()) {
      expect(args.window).toMatchObject({ offset: 52, total: 102, firstMessageId: 'm52', revision: `r${index + 1}` });
      expect(args.data.messages).toHaveLength(50);
      expect(args.data.messages[0].id).toBe('m52');
      expect(args.data.historyWindow).toBeUndefined();
    }
    expect(hook.result.current.messages).toBe(expandedMessages);
    expect(canonical.messages.slice(0, 52)).toEqual(prefix);
    expect(canonical.modelOverride?.model).toBe('queued-second');
    expect(hook.result.current.lastError).toBeNull();
    await model(hook, 'expanded-after-queue');
    expect(saves()).toHaveLength(3);
    expect(saves()[2][1].window).toMatchObject({ offset: 2, total: 102, firstMessageId: 'm2', revision: 'r3' });
    expect(saves()[2][1].data.messages).toHaveLength(100);
    expect(canonical.messages.slice(0, 52)).toEqual(prefix);
    expect(canonical.messages[101].providerMessages).toEqual([{ role: 'assistant', opaque: 'native replay bytes' }]);
    expect(canonical.artifacts[0].id).toBe('unloaded');
    expect(reads()).toHaveLength(2); // Initial tail plus the explicitly requested older page only.
    expect(hook.result.current.lastError).toBeNull();
  });

  it.each(['hidden prefix', 'loaded suffix', 'opaque replay payload', 'unloaded artifact'] as const)(
    'preserves the local snapshot when native rejects a same-ID %s edit during recovery', async (kind) => {
      const hook = await mounted();
      const backend = mocks.invoke.getMockImplementation()!;
      const entered = deferred<void>();
      const release = deferred<void>();
      mocks.invoke.mockImplementation(async (command: string, args: Record<string, any>) => {
        if (command === 'recover_save_ack') {
          entered.resolve();
          await release.promise; // Canonical changes before native checks the receipt hash.
        }
        return backend(command, args);
      });
      const localMessages = hook.result.current.messages;
      const ids = canonical.messages.map((message) => message.id);
      const readCount = reads().length;
      failNextSave = true;
      await act(async () => {
        hook.result.current.selectModel('p', 'committed-before-external-edit');
        await entered.promise;
      });
      expect(saves()).toHaveLength(1);
      expect(canonical.modelOverride?.model).toBe('committed-before-external-edit');
      expect(lastProof).toMatchObject({ sessionId: 'one', offset: 52, total: 102, firstMessageId: 'm52' });
      if (kind === 'hidden prefix') canonical.messages[0].content = 'External same-ID hidden edit';
      if (kind === 'loaded suffix') canonical.messages[75].content = 'External same-ID visible edit';
      if (kind === 'opaque replay payload') canonical.messages[101].providerMessages = [{ role: 'assistant', opaque: 'external replay bytes' }];
      if (kind === 'unloaded artifact') canonical.artifacts[0].content = 'External same-ID artifact edit';
      revision++;
      const externalBytes = JSON.stringify(canonical);
      expect(hash()).not.toBe(lastProof!.canonicalSha256);
      expect(canonical.messages.map((message) => message.id)).toEqual(ids);
      expect(canonical.messages[52].id).toBe(lastProof!.firstMessageId);
      await act(async () => { release.resolve(); });
      expect(recoveries()).toHaveLength(1);
      expect(recoveries()[0][1]).toEqual({ sessionId: 'one', proof: lastProof });
      expect(hook.result.current.lastError).toMatch(/Could not save session:.*Canonical changed after commit/);
      expect(hook.result.current.messages).toBe(localMessages);
      expect(hook.result.current.modelOverride?.model).toBe('committed-before-external-edit');
      expect(JSON.stringify(canonical)).toBe(externalBytes);
      expect(saves()).toHaveLength(1);
      expect(reads()).toHaveLength(readCount);
      expect(mocks.invoke.mock.calls.some(([name]) => name === 'refresh_session_window')).toBe(false);

      await model(hook, 'still-local-after-conflict');
      expect(saves()).toHaveLength(2);
      expect(saves()[1][1].window).toMatchObject({ offset: 52, revision: 'r1', firstMessageId: 'm52' });
      expect(hook.result.current.lastError).toMatch(/stale cursor revision/);
      expect(hook.result.current.modelOverride?.model).toBe('still-local-after-conflict');
      expect(hook.result.current.messages).toBe(localMessages);
      expect(JSON.stringify(canonical)).toBe(externalBytes);
      expect(recoveries()).toHaveLength(1);
      expect(reads()).toHaveLength(readCount);
    },
  );

  it('adopts the exact returned suffix baseline and cursor before pinning and saving again', async () => {
    const hook = await mounted();
    const readCount = reads().length;
    const prefix = structuredClone(canonical.messages.slice(0, 52));
    failNextSave = true;
    await model(hook, 'committed');
    expect(recoveries()).toHaveLength(1);
    expect(recoveries()[0][1]).toEqual({ sessionId: 'one', proof: lastProof });
    expect(hook.result.current.lastError).toBeNull();
    expect(saves()).toHaveLength(1); // Recovery is compare-only, not a retry write.
    await act(async () => { await hook.result.current.togglePinSession('one'); });
    const refresh = mocks.invoke.mock.calls.find(([name]) => name === 'refresh_session_window')![1];
    expect(refresh.window).toMatchObject({ offset: 52, total: 102, revision: 'r2' });
    expect(refresh.data.messages.at(-1).providerMessages).toEqual([{ role: 'assistant', opaque: 'native replay bytes' }]);
    expect(refresh.data.artifacts[0].id).toBe('unloaded');
    expect(saves()).toHaveLength(1); // Unchanged snapshot was acknowledged clean.
    await model(hook, 'after-pin');
    expect(saves()).toHaveLength(2);
    expect(saves()[1][1].window.revision).toBe('r3');
    expect(canonical.modelOverride?.model).toBe('after-pin');
    expect(canonical.messages.slice(0, 52)).toEqual(prefix);
    expect(reads()).toHaveLength(readCount);
    expect(hook.result.current.lastError).toBeNull();
  });
});
