import { pagingBackend } from '../test/sessionPaging';
import { act, cleanup, fireEvent, render, renderHook, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSettings, AttachedContext, SessionData } from '../types/agent';
import { useAgent } from './useAgent';
import { ChatCanvas } from '../components/ChatCanvas';
import type { ComponentProps } from 'react';
vi.mock('../components/AetherLogo', () => ({ AetherLogo: () => <span>Aether</span> }));
vi.mock('../components/MessageRow', () => ({ MessageRow: () => null }));

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn(), showDesktopNotification: vi.fn() }));

const provider = { id: 'vision', name: 'Vision', kind: 'openai' as const, baseUrl: 'http://localhost/v1', apiKey: '', model: 'vision-model' };
const settings: AgentSettings = {
  providers: [provider], activeProviderId: provider.id, systemPrompt: '', temperature: 0.7,
  streamResponse: true, thinking: false, reasoningEffort: 'medium', favoriteModels: [],
  skillPaths: [], mcpServers: [], maxTurns: 24, self_improvement_enabled: false,
};
const initial: SessionData = {
  session: { id: 'image-session', title: 'Images', createdAt: 1, updatedAt: 1, messageCount: 0 },
  messages: [], artifacts: [],
};
const attachment = (path: string, isDir = false): AttachedContext => ({ id: path, path, name: path.split('/').at(-1)!, isDir });
const image = attachment('/workspace/screen.PNG');
const payload = { sourcePath: image.path, mediaType: 'image/png', data: 'cG5n' };
const calls = (name: string) => mocks.invoke.mock.calls.filter(([command]) => command === name);
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function mounted() {
  const hook = renderHook(() => useAgent());
  await waitFor(() => expect(hook.result.current.isLoading).toBe(false));
  return hook;
}
function override(command: string, result: () => unknown) {
  const backend = mocks.invoke.getMockImplementation()!;
  mocks.invoke.mockImplementation(pagingBackend((name: string, args: unknown) => name === command ? result() : backend(name, args)));
}
beforeEach(() => {
  mocks.invoke.mockReset();
  const store = new Map<string, SessionData>([[initial.session.id, structuredClone(initial)]]);
  mocks.invoke.mockImplementation(pagingBackend(async (command: string, args: { sessionId: string; data: SessionData }) => {
    if (command === 'save_session' || command === 'checkpoint_session') {
      store.set(args.data.session.id, structuredClone(args.data));
      return undefined;
    }
    if (command === 'get_settings') return structuredClone(settings);
    if (command === 'list_sessions') return [initial.session];
    if (command === 'load_session') return structuredClone(store.get(args.sessionId));
    if (command === 'prepare_image_attachments') return [payload];
    return undefined;
  }));
});
afterEach(cleanup);

describe('image attachment send preflight', () => {
  it.each([false, true])('persists image bytes and sends the same payload (Council: %s)', async (council) => {
    const hook = await mounted();
    const attachments = [image, attachment('/workspace/notes.md'), attachment('/workspace/folder.png', true)];
    await act(async () => {
      expect(await hook.result.current.sendMessage('Describe this', attachments, {
        council, candidateA: { provider }, candidateB: { provider },
      })).toBe(true);
    });
    expect(calls('prepare_image_attachments')).toEqual([['prepare_image_attachments', { attachments: [image] }]]);
    const commands = mocks.invoke.mock.calls.map(([name]) => name);
    expect(commands.indexOf('prepare_image_attachments')).toBeLessThan(commands.indexOf('save_session_page'));
    expect(hook.result.current.messages[0]).toMatchObject({ content: 'Describe this', attachments, imageInputs: [payload] });
    expect(calls('save_session_page').at(-1)![1].data.messages[0]).toMatchObject({ imageInputs: [payload] });
    const requests = council
      ? [calls('send_council_message')[0][1].requestA, calls('send_council_message')[0][1].requestB]
      : [calls('send_message')[0][1].request];
    for (const request of requests) {
      expect(request.messages[0]).toMatchObject({ imageInputs: [payload] });
      expect(request.messages[0].content).toContain('notes.md');
      expect(request.messages[0].content).toContain('[Directory]');
      expect(request.messages[0].content).not.toContain(payload.data);
    }
  });

  it('forwards restored image history without reading the old file again', async () => {
    const restored = structuredClone(initial);
    restored.messages = [{ id: 'old', role: 'user', content: 'Old image', timestamp: 1, ...{ imageInputs: [payload] } }];
    override('load_session', () => Promise.resolve(restored));
    const hook = await mounted();
    await act(async () => { expect(await hook.result.current.sendMessage('Follow up')).toBe(true); });
    expect(calls('prepare_image_attachments')).toHaveLength(0);
    expect(calls('send_message')[0][1].request.messages[0]).toMatchObject({ imageInputs: [payload] });
    expect(calls('save_session_page').at(-1)![1].data.messages[0]).toMatchObject({ imageInputs: [payload] });
  });

  it('leaves nonimage files and image-named directories as path context without IPC', async () => {
    const hook = await mounted();
    await act(async () => {
      expect(await hook.result.current.sendMessage('', [attachment('/work/readme.txt'), attachment('/work/photos.gif', true)])).toBe(true);
    });
    expect(calls('prepare_image_attachments')).toHaveLength(0);
    expect(calls('send_message')[0][1].request.messages[0].content).toContain('/work/readme.txt');
  });
});

describe('image preflight ownership', () => {
  it.each(['new', 'clear', 'switch', 'model'] as const)('discards prepared bytes after %s changes the origin', async (change) => {
    const hook = await mounted();
    const pending = deferred<typeof payload[]>();
    override('prepare_image_attachments', () => pending.promise);
    let sending!: Promise<boolean>;
    act(() => { sending = hook.result.current.sendMessage('Keep my draft', [image]); });
    expect(hook.result.current.messages).toHaveLength(0);
    expect(hook.result.current.isStreaming).toBe(false);
    if (change === 'new') act(() => { hook.result.current.newSession(); });
    if (change === 'clear') act(() => { hook.result.current.clearCurrentSession(); });
    if (change === 'switch') {
      override('load_session', () => Promise.resolve({ ...initial, session: { ...initial.session, id: 'other' } }));
      await act(async () => { await hook.result.current.switchSession('other'); });
    }
    if (change === 'model') act(() => { hook.result.current.selectModel(provider.id, 'other-model'); });
    await act(async () => { pending.resolve([payload]); expect(await sending).toBe(false); });
    expect(hook.result.current.messages).toHaveLength(0);
    expect(calls('send_message')).toHaveLength(0);
    if (change !== 'model') expect(hook.result.current.lastError).toBeNull();
    else expect(hook.result.current.lastError).toMatch(/model.*changed/i);
    await act(async () => { expect(await hook.result.current.sendMessage('Fresh draft')).toBe(true); });
  });

  it('blocks concurrent sends until image preflight completes', async () => {
    const hook = await mounted();
    const pending = deferred<typeof payload[]>();
    override('prepare_image_attachments', () => pending.promise);
    let sending!: Promise<boolean>;
    act(() => { sending = hook.result.current.sendMessage('First', [image]); });
    await act(async () => { expect(await hook.result.current.sendMessage('Second')).toBe(false); });
    expect(hook.result.current.messages).toHaveLength(0);
    await act(async () => { pending.resolve([payload]); expect(await sending).toBe(true); });
    expect(calls('send_message')).toHaveLength(1);
    expect(hook.result.current.lastError).toBeNull();
  });

  it.each([false, true])('returns false for image failure without late errors or messages (navigated: %s)', async (navigate) => {
    const hook = await mounted();
    const pending = deferred<typeof payload[]>();
    override('prepare_image_attachments', () => pending.promise);
    let sending!: Promise<boolean>;
    act(() => { sending = hook.result.current.sendMessage('Keep draft', [image]); });
    if (navigate) act(() => { hook.result.current.newSession(); });
    await act(async () => { pending.reject(new Error('Selected image is too large')); expect(await sending).toBe(false); });
    expect(hook.result.current.messages).toHaveLength(0);
    expect(calls('send_message')).toHaveLength(0);
    expect(calls('save_session_page')).toHaveLength(0);
    if (navigate) expect(hook.result.current.lastError).toBeNull();
    else expect(hook.result.current.lastError).toMatch(/prepare image.*too large/i);
    await act(async () => { expect(await hook.result.current.sendMessage('Try text instead')).toBe(true); });
  });
});

describe('known text-only model preflight', () => {
  it.each(['deepseek-chat', 'deepseek-reasoner'])('rejects new images for %s before writing messages', async (model) => {
    const hook = await mounted();
    act(() => { hook.result.current.selectModel(provider.id, model); });
    await act(async () => { expect(await hook.result.current.sendMessage('Describe', [image])).toBe(false); });
    expect(hook.result.current.lastError).toMatch(/vision/i);
    expect(hook.result.current.messages).toHaveLength(0);
    expect(calls('prepare_image_attachments')).toHaveLength(0);
    expect(calls('send_message')).toHaveLength(0);
  });

  it.each([false, true])('rejects image history for a text-only normal or Council provider (Council: %s)', async (council) => {
    const restored = structuredClone(initial);
    restored.messages = [{ id: 'old', role: 'user', content: 'Image', timestamp: 1, ...{ imageInputs: [payload] } }];
    override('load_session', () => Promise.resolve(restored));
    const hook = await mounted();
    if (!council) act(() => { hook.result.current.selectModel(provider.id, 'deepseek-chat'); });
    const before = hook.result.current.messages;
    await act(async () => {
      expect(await hook.result.current.sendMessage('Follow up', undefined, {
        council, candidateA: { provider }, candidateB: { provider: { ...provider, model: 'deepseek-reasoner' } },
      })).toBe(false);
    });
    expect(hook.result.current.lastError).toMatch(/vision/i);
    expect(hook.result.current.messages).toBe(before);
    expect(calls('send_message')).toHaveLength(0);
    expect(calls('send_council_message')).toHaveLength(0);
  });
});

describe('image payload reuse after a turn', () => {
  it.each(['retry', 'edit', 'undo'] as const)('reuses persisted image bytes for %s after the source disappears', async (operation) => {
    const hook = await mounted();
    override('send_message', () => Promise.reject(new Error('Provider unavailable')));
    await act(async () => { await hook.result.current.sendMessage('Original prompt', [image]); });
    const messageId = hook.result.current.messages[0].id;
    expect(calls('prepare_image_attachments')).toHaveLength(1);
    override('prepare_image_attachments', () => Promise.reject(new Error('File no longer exists')));
    override('send_message', () => Promise.resolve(undefined));
    await act(async () => {
      if (operation === 'retry') expect(await hook.result.current.retryTurn(messageId)).toBe(true);
      if (operation === 'edit') expect(await hook.result.current.editAndResendMessage(messageId, 'Edited prompt')).toBe(true);
      if (operation === 'undo') {
        const undone = await hook.result.current.undoTurn();
        expect(undone).toMatchObject({ imageInputs: [payload] });
        expect(await hook.result.current.sendMessage(undone!.content, undone!.attachments, {
          imageInputs: undone!.imageInputs,
        })).toBe(true);
      }
    });
    expect(calls('prepare_image_attachments')).toHaveLength(1);
    const request = calls('send_message').at(-1)![1].request;
    expect(request.messages.at(-1)).toMatchObject({
      content: expect.stringContaining(operation === 'edit' ? 'Edited prompt' : 'Original prompt'),
      imageInputs: [payload],
    });
    expect(request.messages.at(-1).content).not.toContain(payload.data);
    expect(hook.result.current.messages[0].imageInputs).toEqual([payload]);
  });
});

describe('empty-text image turn replay', () => {
  it('retries image-only user and assistant turns with stored bytes after the source disappears', async () => {
    for (const role of ['user', 'assistant'] as const) {
      const hook = await mounted();
      override('send_message', () => Promise.reject(new Error('Provider unavailable')));
      await act(async () => { expect(await hook.result.current.sendMessage('', [image])).toBe(true); });
      const original = hook.result.current.messages[0];
      const target = hook.result.current.messages.find((message) => message.role === role)!;
      expect(original).toMatchObject({ content: '', attachments: [image], imageInputs: [payload] });
      expect(target).toBeDefined();
      const preparations = calls('prepare_image_attachments').length;
      const sends = calls('send_message').length;
      override('prepare_image_attachments', () => Promise.reject(new Error('File no longer exists')));
      override('send_message', () => Promise.resolve(undefined));
      await act(async () => { expect(await hook.result.current.retryTurn(target.id)).toBe(true); });
      expect(calls('prepare_image_attachments')).toHaveLength(preparations);
      expect(calls('send_message')).toHaveLength(sends + 1);
      expect(hook.result.current.messages[0]).toMatchObject({ content: '', imageInputs: [payload] });
      expect(hook.result.current.messages[0].attachments).toBe(original.attachments);
      expect(calls('save_session').at(-1)![1].data.messages[0]).toMatchObject({ attachments: [image], imageInputs: [payload] });
      expect(calls('send_message').at(-1)![1].request.messages[0]).toMatchObject({ imageInputs: [payload] });
      hook.unmount();
      override('prepare_image_attachments', () => Promise.resolve([payload]));
    }
  });

  it('allows clearing image-turn text but rejects an empty text-only edit without changing history', async () => {
    const restored = structuredClone(initial);
    restored.messages = [
      { id: 'image-turn', role: 'user', content: 'Describe this', timestamp: 1, attachments: [image], imageInputs: [payload] },
      { id: 'answer', role: 'assistant', content: 'An image', timestamp: 2 },
      { id: 'text-turn', role: 'user', content: 'Follow up', timestamp: 3 },
    ];
    override('load_session', () => Promise.resolve(restored));
    override('prepare_image_attachments', () => Promise.reject(new Error('File no longer exists')));
    const hook = await mounted();
    const before = hook.result.current.messages;
    const snapshot = structuredClone(before);
    const saves = calls('save_session').length + calls('save_session_page').length;
    await act(async () => { expect(await hook.result.current.editAndResendMessage('text-turn', '   ')).toBe(false); });
    expect(hook.result.current.messages).toBe(before);
    expect(hook.result.current.messages).toEqual(snapshot);
    expect(calls('save_session').length + calls('save_session_page').length).toBe(saves);
    expect(calls('send_message')).toHaveLength(0);
    await act(async () => { expect(await hook.result.current.editAndResendMessage('image-turn', '   ')).toBe(true); });
    expect(hook.result.current.messages[0]).toMatchObject({ content: '', imageInputs: [payload] });
    expect(hook.result.current.messages[0].attachments).toBe(before[0].attachments);
    expect(hook.result.current.messages.some((message) => message.id === 'text-turn')).toBe(false);
    expect(calls('prepare_image_attachments')).toHaveLength(0);
    expect(calls('save_session').at(-1)![1].data.messages[0]).toMatchObject({ attachments: [image], imageInputs: [payload] });
    expect(calls('send_message')[0][1].request.messages[0]).toMatchObject({ imageInputs: [payload] });
  });
});

describe('undo composer image ownership', () => {
  function canvas() {
    const props: ComponentProps<typeof ChatCanvas> = {
      messages: [], isStreaming: false, stopRequested: false,
      onSendMessage: vi.fn(async () => true), onStop: vi.fn(), onSelectArtifact: vi.fn(),
      onClearSession: vi.fn(), onNewSession: () => 'new', onOpenSettings: vi.fn(), onOpenModelPicker: vi.fn(),
      effectiveConfig: null, sessionTitle: 'Images', currentSessionId: 'images', artifactCount: 0,
      isArtifactOpen: false, onToggleArtifacts: vi.fn(), onListFiles: async () => [],
      onUndoTurn: vi.fn(async () => ({ content: 'Original', attachments: [image], imageInputs: [payload] })),
    };
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
    return { props, ...render(<ChatCanvas {...props} />) };
  }
  function submit(text: string) {
    const input = screen.getByLabelText('Message AetherAgent');
    fireEvent.change(input, { target: { value: text } });
    fireEvent.keyDown(input, { key: 'Escape' });
    fireEvent.keyDown(input, { key: 'Enter' });
  }
  it('forwards saved bytes through slash retry', async () => {
    const { props } = canvas();
    submit('/retry');
    await waitFor(() => expect(props.onSendMessage).toHaveBeenCalledWith('Original', [image], { imageInputs: [payload] }));
  });
  it.each(['send', 'remove', 'queue', 'edit queue'] as const)('keeps undo bytes scoped to the draft during %s', async (operation) => {
    const { props, rerender } = canvas();
    submit('/undo');
    await waitFor(() => expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('Original'));
    if (operation === 'remove') fireEvent.click(screen.getByRole('button', { name: 'Remove screen.PNG' }));
    if (operation === 'queue' || operation === 'edit queue') rerender(<ChatCanvas {...props} isStreaming />);
    submit('Edited');
    if (operation === 'edit queue') {
      fireEvent.click(screen.getByTitle('Edit prompt in composer'));
      rerender(<ChatCanvas {...props} />);
      submit('Edited again');
    } else if (operation === 'queue') rerender(<ChatCanvas {...props} />);
    await waitFor(() => expect(props.onSendMessage).toHaveBeenCalledOnce());
    const args = vi.mocked(props.onSendMessage).mock.calls[0];
    if (operation === 'remove') {
      expect(args[1]).toBeUndefined();
      expect(args[2]?.imageInputs).toBeUndefined();
    } else {
      expect(args[1]).toEqual([image]);
      expect(args[2]?.imageInputs).toEqual([payload]);
    }
  });
});

describe('supplied image payload selection', () => {
  it('prepares only newly attached images and snapshots supplied bytes before awaiting IPC', async () => {
    const hook = await mounted();
    const added = attachment('/workspace/new.jpg');
    const freshPayload = { sourcePath: added.path, mediaType: 'image/jpeg', data: 'bmV3' };
    const supplied = { ...payload };
    const pending = deferred<typeof payload[]>();
    override('prepare_image_attachments', () => pending.promise);
    let sending!: Promise<boolean>;
    act(() => {
      sending = hook.result.current.sendMessage('Both images', [image, added], { imageInputs: [supplied] });
    });
    supplied.data = 'mutated';
    expect(calls('prepare_image_attachments')).toEqual([['prepare_image_attachments', { attachments: [added] }]]);
    await act(async () => { pending.resolve([freshPayload]); expect(await sending).toBe(true); });
    expect(hook.result.current.messages[0].imageInputs).toEqual([payload, freshPayload]);
    expect(calls('send_message')[0][1].request.messages[0].imageInputs).toEqual([payload, freshPayload]);
  });

  it('does not inject a supplied image when its source is not a selected image file', async () => {
    const hook = await mounted();
    await act(async () => {
      expect(await hook.result.current.sendMessage('Text only', [attachment(image.path, true)], { imageInputs: [payload] })).toBe(true);
    });
    expect(hook.result.current.messages[0].imageInputs).toBeUndefined();
    expect(calls('prepare_image_attachments')).toHaveLength(0);
    expect(calls('send_message')[0][1].request.messages[0].imageInputs).toBeUndefined();
  });

  it('rejects supplied images on known text-only models without reading the file', async () => {
    const hook = await mounted();
    act(() => { hook.result.current.selectModel(provider.id, 'deepseek-chat'); });
    await act(async () => {
      expect(await hook.result.current.sendMessage('Describe', [image], { imageInputs: [payload] })).toBe(false);
    });
    expect(hook.result.current.lastError).toMatch(/vision/i);
    expect(calls('prepare_image_attachments')).toHaveLength(0);
    expect(calls('send_message')).toHaveLength(0);
  });
});


describe('native lightweight image history', () => {
  async function lightweight() {
    const full = structuredClone(initial);
    full.messages = [
      { id: 'old-image', role: 'user', content: 'Original', timestamp: 1, attachments: [image], imageInputs: [payload] },
      { id: 'answer', role: 'assistant', content: 'Answer', timestamp: 2 },
    ];
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation(async (command: string, args: any) => {
      if (command === 'load_session_page') return {
        data: { ...full, messages: full.messages.map(({ imageInputs, ...message }) => ({ ...message, ...(imageInputs ? { hasImageInputs: true } : {}) })) },
        window: { offset: 0, total: 2, revision: 'thin', firstMessageId: 'old-image' },
      };
      if (command === 'load_session') return structuredClone(full);
      if (command === 'read_message_image_inputs') return [payload];
      if (command === 'prepare_image_attachments') throw new Error('File deleted');
      return backend(command, args);
    });
    return mounted();
  }
  it.each(['retry', 'edit', 'undo'] as const)('restores canonical bytes before destructive %s', async (operation) => {
    const hook = await lightweight();
    expect(hook.result.current.messages[0].imageInputs).toBeUndefined();
    expect(hook.result.current.messages[0].hasImageInputs).toBe(true);
    expect(calls('load_session')).toHaveLength(0);
    await act(async () => {
      if (operation === 'retry') expect(await hook.result.current.retryTurn('answer')).toBe(true);
      if (operation === 'edit') expect(await hook.result.current.editAndResendMessage('old-image', 'Edited')).toBe(true);
      if (operation === 'undo') {
        const undone = await hook.result.current.undoTurn();
        expect(undone?.imageInputs).toEqual([payload]);
        expect(await hook.result.current.sendMessage(undone!.content, undone!.attachments, { imageInputs: undone!.imageInputs })).toBe(true);
      }
    });
    expect(calls('prepare_image_attachments')).toHaveLength(0);
    expect(calls('send_message').at(-1)![1].request.messages.at(-1).imageInputs).toEqual([payload]);
    const commands = mocks.invoke.mock.calls.map(([name]) => name);
    if (operation === 'undo') expect(calls('read_message_image_inputs')).toHaveLength(0);
    else {
      expect(calls('read_message_image_inputs')[0]).toEqual(['read_message_image_inputs', { sessionId: initial.session.id, messageId: 'old-image' }]);
      expect(commands.indexOf('read_message_image_inputs')).toBeLessThan(commands.indexOf('save_session'));
    }
  });
  it('abandons delayed exact hydration after navigation without truncating either session', async () => {
    const hook = await lightweight();
    const pending = deferred<typeof payload[]>();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: any) => command === 'read_message_image_inputs' ? pending.promise : backend(command, args));
    let retry!: Promise<boolean>;
    act(() => { retry = hook.result.current.retryTurn('old-image'); });
    expect(calls('read_message_image_inputs')).toHaveLength(1);
    act(() => { hook.result.current.newSession(); });
    const current = hook.result.current.messages;
    await act(async () => { pending.resolve([payload]); expect(await retry).toBe(false); });
    expect(hook.result.current.messages).toBe(current);
    expect(calls('send_message')).toHaveLength(0);
    expect(calls('load_session')).toHaveLength(0);
  });
  it('sends a follow-up from thin history without hydrating historical images', async () => {
    const hook = await lightweight();
    await act(async () => { expect(await hook.result.current.sendMessage('Follow up')).toBe(true); });
    expect(calls('load_session')).toHaveLength(0);
    expect(calls('read_message_image_inputs')).toHaveLength(0);
    expect(calls('prepare_image_attachments')).toHaveLength(0);
    const request = calls('send_message')[0][1].request;
    expect(request.messages[0].hasImageInputs).toBe(true);
    expect(request.messages[0].imageInputs).toBeUndefined();
  });
  it('does not truncate when exact hydration fails', async () => {
    const hook = await lightweight();
    const backend = mocks.invoke.getMockImplementation()!;
    mocks.invoke.mockImplementation((command: string, args: any) => command === 'read_message_image_inputs' ? Promise.reject(new Error('Unavailable')) : backend(command, args));
    const before = hook.result.current.messages;
    await act(async () => { expect(await hook.result.current.retryTurn('old-image')).toBe(false); });
    expect(hook.result.current.messages).toBe(before);
    expect(calls('save_session')).toHaveLength(0);
    expect(calls('send_message')).toHaveLength(0);
  });
  it('checks text-only model preflight using the marker without hydrating history', async () => {
    const hook = await lightweight();
    await act(async () => { expect(await hook.result.current.sendMessage('Follow up', undefined, { council: true, candidateA: { provider }, candidateB: { provider: { ...provider, model: 'deepseek-chat' } } })).toBe(false); });
    expect(hook.result.current.lastError).toMatch(/vision/i);
    expect(calls('read_message_image_inputs')).toHaveLength(0);
    expect(calls('load_session')).toHaveLength(0);
  });
});
