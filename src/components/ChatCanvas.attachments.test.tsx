import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ComponentProps } from 'react';
import { ChatCanvas } from './ChatCanvas';
import { open } from '@tauri-apps/plugin-dialog';
import type { AttachedContext } from '../types/agent';

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ onDragDropEvent: native.listen }) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./AetherLogo', () => ({ AetherLogo: () => <span>Aether</span> }));
vi.mock('./MessageRow', () => ({ MessageRow: () => null }));

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
});
beforeEach(() => {
  (window as any).__TAURI_INTERNALS__ = {};
  native.invoke.mockReset();
  vi.mocked(open).mockReset();
  native.listen.mockReset().mockResolvedValue(vi.fn());
});
afterEach(() => { delete (window as any).__TAURI_INTERNALS__; });

type Props = ComponentProps<typeof ChatCanvas> & { workspaceCwd?: string };
function setup(overrides: Partial<Props> = {}) {
  const props: Props = {
    messages: [], isStreaming: false, stopRequested: false,
    onSendMessage: vi.fn(async () => true), onStop: vi.fn(), onSelectArtifact: vi.fn(),
    onClearSession: vi.fn(), onNewSession: () => 'new', onOpenSettings: vi.fn(),
    onOpenModelPicker: vi.fn(), effectiveConfig: null, sessionTitle: 'Attachments',
    artifactCount: 0, isArtifactOpen: false, onToggleArtifacts: vi.fn(), onListFiles: async () => [],
    currentSessionId: 'session-a', workspaceCwd: 'C:/workspace-a', ...overrides,
  };
  const view = render(<ChatCanvas {...props} />);
  return { ...view, props, changeScope: (next: Partial<Props>) => view.rerender(<ChatCanvas {...props} {...next} />) };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const context: AttachedContext = { id: 'old-file', name: 'origin.png', path: 'C:/workspace-a/origin.png', isDir: false };
function file(path?: string, read: () => Promise<ArrayBuffer> = async () => new Uint8Array([1, 2]).buffer) {
  const value = new File(['image'], 'origin.png', { type: 'image/png' });
  Object.defineProperty(value, 'arrayBuffer', { value: read });
  if (path) Object.defineProperty(value, 'path', { value: path });
  return value;
}
function paste(value = file()) {
  fireEvent.paste(screen.getByRole('textbox'), { clipboardData: { files: [value], items: [] } });
}

describe('attachment origin and preparation', () => {
  it.each(['paste', 'drop'])('rejects oversized native %s before reading or serializing bytes', async (method) => {
    const read = vi.fn(async () => new Uint8Array([1]).buffer);
    const oversized = file(undefined, read);
    native.invoke.mockResolvedValue(context);
    Object.defineProperty(oversized, 'size', { value: 25 * 1024 * 1024 + 1 });
    const view = setup();
    if (method === 'paste') paste(oversized);
    else fireEvent.drop(view.container.firstChild!, { dataTransfer: { files: [oversized] } });
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('25 MiB'));
    expect(read).not.toHaveBeenCalled();
    expect(native.invoke).not.toHaveBeenCalled();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('does not read browser-only placeholder attachment bytes', async () => {
    delete (window as any).__TAURI_INTERNALS__;
    setup();
    const read = vi.fn(async () => new Uint8Array([1]).buffer);
    paste(file(undefined, read));
    await screen.findByText('origin.png');
    expect(read).not.toHaveBeenCalled();
    expect(native.invoke).not.toHaveBeenCalled();
  });
  it.each([
    { currentSessionId: 'session-b' },
    { workspaceCwd: 'C:/workspace-b' },
  ])('ignores a saved paste after origin changes: %j', async (next) => {
    const saved = deferred<AttachedContext>();
    native.invoke.mockReturnValue(saved.promise);
    const view = setup();
    paste();
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith('save_pasted_attachment', expect.any(Object)));
    view.changeScope(next);
    await act(async () => saved.resolve(context));
    expect(screen.queryByText('origin.png')).toBeNull();
  });

  it('ignores an inspected HTML drop after session navigation', async () => {
    const inspected = deferred<AttachedContext[]>();
    native.invoke.mockReturnValue(inspected.promise);
    const view = setup();
    fireEvent.drop(view.container.firstChild!, { dataTransfer: { files: [file(context.path)] } });
    expect(native.invoke).toHaveBeenCalledWith('inspect_paths', { paths: [context.path] });
    view.changeScope({ currentSessionId: 'session-b' });
    await act(async () => inspected.resolve([context]));
    expect(screen.queryByText('origin.png')).toBeNull();
  });

  it('blocks button and Enter sends until paste preparation is complete', async () => {
    const saved = deferred<AttachedContext>();
    native.invoke.mockReturnValue(saved.promise);
    const { props } = setup();
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Describe this image' } });
    paste();
    await waitFor(() => expect(native.invoke).toHaveBeenCalledOnce());
    const send = screen.getByRole('button', { name: 'Send message' }) as HTMLButtonElement;
    expect(send.disabled).toBe(true);
    expect(screen.getByRole('status').textContent).toMatch(/preparing attachment/i);
    fireEvent.click(send);
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });
    expect(props.onSendMessage).not.toHaveBeenCalled();
    await act(async () => saved.resolve(context));
    expect(send.disabled).toBe(false);
    fireEvent.click(send);
    await waitFor(() => expect(props.onSendMessage).toHaveBeenCalledWith('Describe this image', [context], expect.any(Object)));
  });
  it('captures expected workspace before reading bytes and stops after navigation', async () => {
    const read = deferred<ArrayBuffer>();
    const view = setup();
    paste(file(undefined, () => read.promise));
    view.changeScope({ workspaceCwd: 'C:/workspace-b' });
    await act(async () => read.resolve(new Uint8Array([1]).buffer));
    expect(native.invoke).not.toHaveBeenCalled();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it.each(['C:/workspace-a', ''])('passes captured expected workspace %j to native save', async (workspaceCwd) => {
    native.invoke.mockResolvedValue(context);
    setup({ workspaceCwd });
    paste();
    await waitFor(() => expect(native.invoke).toHaveBeenCalledWith('save_pasted_attachment', {
      name: 'origin.png', bytes: [1, 2], expectedWorkspace: workspaceCwd,
    }));
    await waitFor(() => expect(screen.getByText('origin.png')).toBeTruthy());
  });

  it.each(['event', 'button'])('invalidates pending paste on explicit composer clear via %s', async (method) => {
    const saved = deferred<AttachedContext>();
    native.invoke.mockReturnValue(saved.promise);
    setup();
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'draft' } });
    paste();
    await waitFor(() => expect(native.invoke).toHaveBeenCalledOnce());
    if (method === 'event') act(() => window.dispatchEvent(new Event('aether:clear-composer')));
    else fireEvent.click(screen.getByRole('button', { name: 'Clear composer' }));
    await act(async () => saved.resolve(context));
    expect(screen.queryByText('origin.png')).toBeNull();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('shows current native errors without a fake attachment fallback', async () => {
    native.invoke.mockRejectedValue(new Error('Workspace changed'));
    const view = setup();
    fireEvent.drop(view.container.firstChild!, { dataTransfer: { files: [file()] } });
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('Workspace changed'));
    expect(screen.queryByText('origin.png')).toBeNull();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('ignores a stale error without finishing the new generation preparation', async () => {
    const old = deferred<AttachedContext>();
    const current = deferred<AttachedContext>();
    native.invoke.mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
    const view = setup();
    paste();
    await waitFor(() => expect(native.invoke).toHaveBeenCalledTimes(1));
    view.changeScope({ currentSessionId: 'session-b' });
    paste();
    await waitFor(() => expect(native.invoke).toHaveBeenCalledTimes(2));
    await act(async () => old.reject(new Error('stale failure')));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.getByRole('status').textContent).toContain('(1)');
    await act(async () => current.resolve(context));
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.getByText('origin.png')).toBeTruthy();
  });

  it('waits for all concurrent preparations before enabling Send', async () => {
    const first = deferred<AttachedContext>();
    const second = deferred<AttachedContext>();
    native.invoke.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    setup();
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'draft' } });
    paste();
    paste();
    await waitFor(() => expect(native.invoke).toHaveBeenCalledTimes(2));
    expect(screen.getByRole('status').textContent).toContain('(2)');
    await act(async () => first.resolve(context));
    expect((screen.getByRole('button', { name: 'Send message' }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole('status').textContent).toContain('(1)');
    await act(async () => second.resolve(context));
    expect(screen.queryByRole('status')).toBeNull();
    expect(screen.getAllByText('origin.png')).toHaveLength(1);
  });

  it('guards native drop inspection with the event origin', async () => {
    const inspected = deferred<AttachedContext[]>();
    native.invoke.mockReturnValue(inspected.promise);
    const view = setup();
    await waitFor(() => expect(native.listen).toHaveBeenCalledOnce());
    const handler = native.listen.mock.calls[0][0];
    let completion!: Promise<void>;
    act(() => { completion = handler({ payload: { type: 'drop', paths: [context.path] } }); });
    view.changeScope({ currentSessionId: 'session-b' });
    await act(async () => { inspected.resolve([context]); await completion; });
    expect(screen.queryByText('origin.png')).toBeNull();
  });

  it('invalidates paste begun while an accepted send is awaiting completion', async () => {
    const accepted = deferred<boolean>();
    const saved = deferred<AttachedContext>();
    native.invoke.mockReturnValue(saved.promise);
    setup({ onSendMessage: vi.fn(() => accepted.promise) });
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'draft' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }));
    paste();
    await waitFor(() => expect(native.invoke).toHaveBeenCalledOnce());
    await act(async () => accepted.resolve(true));
    await act(async () => saved.resolve(context));
    expect(screen.queryByText('origin.png')).toBeNull();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it('does not attach file-dialog results after navigation', async () => {
    const selected = deferred<string[] | null>();
    vi.mocked(open).mockReturnValue(selected.promise as ReturnType<typeof open>);
    const view = setup();
    fireEvent.click(screen.getByRole('button', { name: 'Add Context' }));
    fireEvent.click(screen.getByRole('button', { name: /^Files$/ }));
    view.changeScope({ currentSessionId: 'session-b' });
    await act(async () => selected.resolve([context.path]));
    expect(screen.queryByText('origin.png')).toBeNull();
  });

});

describe('attachment format disclosure', () => {
  it.each(['SVG', 'bmp', 'tif', 'tiff', 'heic', 'heif', 'avif'])('discloses %s as file context without blocking Send and clears the notice on removal', async (extension) => {
    const unsupported = { ...context, id: 'unsupported', name: `diagram.${extension}`, path: `C:/workspace-a/diagram.${extension}` };
    native.invoke.mockResolvedValue([unsupported, context]);
    const view = setup();
    fireEvent.drop(view.container.firstChild!, { dataTransfer: { files: [file(unsupported.path), file(context.path)] } });
    await waitFor(() => expect(screen.getByText(unsupported.name)).toBeTruthy());
    const note = screen.getByRole('note');
    expect(note.textContent).toContain('pixels are not sent for vision');
    expect(note.textContent).toContain('PNG, JPEG, GIF, and WebP only');
    expect(note.textContent).toContain('SVG files can still be read as text by tools');
    expect(note.title).toBe('Unsupported image format: attached as file context, not sent as pixels for vision.');
    expect((screen.getByRole('button', { name: 'Send message' }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: `Remove ${unsupported.name}` }));
    expect(screen.queryByRole('note')).toBeNull();
    expect(screen.getByText('origin.png')).toBeTruthy();
  });

  it.each(['png', 'jpg', 'jpeg', 'gif', 'webp'])('does not warn for a native supported %s attachment', async (extension) => {
    const supported = { ...context, name: `photo.${extension}`, path: `C:/workspace-a/photo.${extension}` };
    native.invoke.mockResolvedValue([supported]);
    const view = setup();
    fireEvent.drop(view.container.firstChild!, { dataTransfer: { files: [file(supported.path)] } });
    await waitFor(() => expect(screen.getByText(supported.name)).toBeTruthy());
    expect(screen.queryByRole('note')).toBeNull();
  });

  it('does not warn for a directory with an unsupported image extension', async () => {
    const directory = { ...context, name: 'assets.svg', path: 'C:/workspace-a/assets.svg', isDir: true };
    native.invoke.mockResolvedValue([directory]);
    const view = setup();
    fireEvent.drop(view.container.firstChild!, { dataTransfer: { files: [file(directory.path)] } });
    await waitFor(() => expect(screen.getByText(directory.name)).toBeTruthy());
    expect(screen.queryByRole('note')).toBeNull();
  });
});
it('wires preview controls into the existing chips, without reading bytes before a click', async () => {
  native.invoke.mockImplementation(async (command) => command === 'prepare_image_attachments'
    ? [{ sourcePath: context.path, mediaType: 'image/png', data: 'iVBORw0KGgo=' }] : [context]);
  const view = setup();
  fireEvent.drop(view.container.firstChild!, { dataTransfer: { files: [file(context.path)] } });
  const trigger = await screen.findByRole('button', { name: 'Preview origin.png' });
  expect(screen.getByText('Vision input')).toBeTruthy();
  expect(native.invoke.mock.calls.filter(([command]) => command === 'prepare_image_attachments')).toHaveLength(0);
  fireEvent.click(trigger);
  await waitFor(() => expect(screen.getByRole('dialog').querySelector('img')).toBeTruthy());
  fireEvent.keyDown(document, { key: 'Escape' });
  fireEvent.click(screen.getByRole('button', { name: 'Remove origin.png' }));
  expect(screen.queryByRole('button', { name: 'Preview origin.png' })).toBeNull();
});

it('limits preview controls to four images while retaining every attachment', async () => {
  const images = Array.from({ length: 5 }, (_, index) => ({ ...context, id: String(index), name: `${index}.png`, path: `C:/workspace-a/${index}.png` }));
  native.invoke.mockResolvedValue(images);
  const view = setup();
  fireEvent.drop(view.container.firstChild!, { dataTransfer: { files: images.map((image) => file(image.path)) } });
  await screen.findByText('4.png');
  expect(screen.getAllByRole('button', { name: /^Preview .*png$/ })).toHaveLength(4);
  expect(screen.getAllByRole('button', { name: /^Remove .*png$/ })).toHaveLength(5);
});

it('keeps attachment preview selection linear while typing with many file contexts', async () => {
  let pathReads = 0;
  const files = Array.from({ length: 80 }, (_, index) => ({
    id: `file-${index}`, name: `${index}.txt`, isDir: false,
    get path() { pathReads += 1; return `C:/workspace-a/${index}.txt`; },
  }));
  native.invoke.mockResolvedValue(files);
  const view = setup();
  fireEvent.drop(view.container.firstChild!, { dataTransfer: { files: [file(context.path)] } });
  await screen.findByText('79.txt');
  pathReads = 0;
  fireEvent.change(screen.getByRole('textbox'), { target: { value: 'next draft' } });
  expect(pathReads).toBeLessThan(files.length * 12);
  expect(screen.getAllByRole('button', { name: /^Remove .*txt$/ })).toHaveLength(80);
  expect(screen.queryByRole('button', { name: /^Preview/ })).toBeNull();
});

it('updates the vision indicator when the selected model becomes text-only', async () => {
  native.invoke.mockResolvedValue([context]);
  const view = setup();
  fireEvent.drop(view.container.firstChild!, { dataTransfer: { files: [file(context.path)] } });
  await screen.findByText('Vision input');
  view.changeScope({ effectiveConfig: {
    provider: { id: 'custom', name: 'Custom', kind: 'openai', baseUrl: '', apiKey: '', model: 'deepseek-chat' },
    thinking: false, reasoningEffort: 'medium',
  } as NonNullable<Props['effectiveConfig']> });
  expect(screen.getByText('Vision blocked: text-only model')).toBeTruthy();
  expect(screen.queryByText('File context')).toBeNull();
  expect(screen.getByRole('button', { name: 'Preview origin.png' })).toBeTruthy();
  expect(native.invoke.mock.calls.filter(([command]) => command === 'prepare_image_attachments')).toHaveLength(0);
});
