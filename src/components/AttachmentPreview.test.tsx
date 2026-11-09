import { Buffer } from 'node:buffer';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { AttachmentPreview } from './AttachmentPreview';
import type { AttachedContext } from '../types/agent';

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }));
const attachment: AttachedContext = { id: 'image', name: 'photo.png', path: 'C:/workspace/photo.png', isDir: false };
const payload = { sourcePath: attachment.path, mediaType: 'image/png', data: 'iVBORw0KGgo=' };
const props = { attachment, sessionId: 'session-a', workspaceCwd: 'C:/workspace', model: 'gpt-4o' };
beforeEach(() => { native.invoke.mockReset().mockResolvedValue([payload]); });

describe('AttachmentPreview', () => {
  it('loads an explicit image only on a preview gesture, retains its thumbnail, and restores focus on Escape', async () => {
    render(<AttachmentPreview {...props} />);
    expect(screen.getByText('Vision input')).toBeTruthy();
    expect(native.invoke).not.toHaveBeenCalled();
    const trigger = screen.getByRole('button', { name: 'Preview photo.png' });
    trigger.focus();
    fireEvent.click(trigger);
    await waitFor(() => expect(screen.getByRole('dialog').querySelector('img')?.getAttribute('src')).toMatch(/^(blob:|data:image\/png;base64,)/));
    expect(native.invoke).toHaveBeenCalledExactlyOnceWith('prepare_image_attachments', { attachments: [attachment] });
    expect(screen.getByRole('dialog').querySelector('img')?.alt).toBe('photo.png');
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    expect(screen.getByRole('img', { name: 'photo.png' })).toBeTruthy();
    fireEvent.click(trigger);
    await waitFor(() => expect(screen.getByRole('dialog').querySelector('img')).toBeTruthy());
    expect(native.invoke).toHaveBeenCalledTimes(1);
  });
});

it('disables preview controls outside the bounded thumbnail set', () => {
  render(<AttachmentPreview {...props} allowPreview={false} />);
  expect(screen.queryByRole('button')).toBeNull();
  expect(screen.getByText('Vision input')).toBeTruthy();
  expect(native.invoke).not.toHaveBeenCalled();
});

it('renders a full bounded native payload without a regex stack overflow', async () => {
  native.invoke.mockResolvedValue([{ ...payload, data: btoa('x'.repeat(1024 * 1024)) }]);
  render(<AttachmentPreview {...props} sessionId="large" />);
  fireEvent.click(screen.getByRole('button'));
  await waitFor(() => expect(screen.getByRole('dialog').querySelector('img')).toBeTruthy());
  expect(screen.queryByRole('alert')).toBeNull();
});

it.each(['svg', 'bmp', 'txt'])('never fetches unsupported %s files', (extension) => {
  render(<AttachmentPreview {...props} attachment={{ ...attachment, path: `C:/workspace/file.${extension}`, name: `file.${extension}` }} />);
  expect(screen.getByText('File context')).toBeTruthy();
  expect(screen.queryByRole('button')).toBeNull();
  expect(native.invoke).not.toHaveBeenCalled();
});

it.each(['deepseek-chat', 'DeepSeek-Reasoner'])('discloses blocked vision for %s, not file context', (model) => {
  render(<AttachmentPreview {...props} model={model} />);
  expect(screen.getByText('Vision blocked: text-only model')).toBeTruthy();
  expect(screen.queryByText('File context')).toBeNull();
});

it.each([{ sessionId: 'changed-session' }, { workspaceCwd: 'C:/changed' }])('ignores pending results after origin changes: %j', async (next) => {
  let resolve!: (value: unknown) => void;
  native.invoke.mockReturnValue(new Promise((yes) => { resolve = yes; }));
  const view = render(<AttachmentPreview {...props} sessionId="pending" />);
  fireEvent.click(screen.getByRole('button'));
  view.rerender(<AttachmentPreview {...props} {...next} />);
  await act(async () => resolve([payload]));
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(screen.queryByRole('img')).toBeNull();
});

it('revokes a loaded Blob URL on unmount', async () => {
  const create = vi.fn(() => 'blob:preview-owned');
  const revoke = vi.fn();
  vi.stubGlobal('URL', { createObjectURL: create, revokeObjectURL: revoke });
  try {
    const view = render(<AttachmentPreview {...props} sessionId="revoke" />);
    fireEvent.click(screen.getByRole('button'));
    await waitFor(() => expect(create).toHaveBeenCalledOnce());
    view.unmount();
    expect(revoke).toHaveBeenCalledExactlyOnceWith('blob:preview-owned');
  } finally { vi.unstubAllGlobals(); }
});

it.each([
  { ...payload, mediaType: 'image/svg+xml' },
  { ...payload, sourcePath: 'C:/other.png' },
  { ...payload, data: 'https://example.com/image.png' },
])('rejects unsafe preview payload: %j', async (bad) => {
  native.invoke.mockResolvedValue([bad]);
  render(<AttachmentPreview {...props} sessionId={JSON.stringify(bad)} />);
  fireEvent.click(screen.getByRole('button'));
  expect((await screen.findByRole('alert')).textContent).toContain('Invalid image preview payload');
  expect(screen.queryByRole('img')).toBeNull();
});

it('shows a preparation error without falling back to a local asset URL', async () => {
  native.invoke.mockRejectedValue(new Error('Cannot inspect selected image'));
  render(<AttachmentPreview {...props} sessionId="failed" />);
  fireEvent.click(screen.getByRole('button'));
  expect((await screen.findByRole('alert')).textContent).toContain('Cannot inspect selected image');
  expect(screen.queryByRole('img')).toBeNull();
});

it.each(['image/jpeg', 'image/gif', 'image/webp'])('renders the native %s media type', async (mediaType) => {
  vi.stubGlobal('URL', {});
  native.invoke.mockResolvedValue([{ ...payload, mediaType }]);
  render(<AttachmentPreview {...props} sessionId={mediaType} />);
  fireEvent.click(screen.getByRole('button'));
  await waitFor(() => expect(screen.getByRole('dialog').querySelector('img')?.getAttribute('src')).toContain(`data:${mediaType};base64,`));
  vi.unstubAllGlobals();
});

it('evicts preview cache entries beyond eight', async () => {
  for (let index = 0; index < 9; index += 1) {
    const view = render(<AttachmentPreview {...props} sessionId={`cache-count-${index}`} />);
    fireEvent.click(screen.getByRole('button'));
    await waitFor(() => expect(screen.getByRole('dialog').querySelector('img')).toBeTruthy());
    view.unmount();
  }
  const count = native.invoke.mock.calls.length;
  render(<AttachmentPreview {...props} sessionId="cache-count-0" />);
  fireEvent.click(screen.getByRole('button'));
  await waitFor(() => expect(native.invoke).toHaveBeenCalledTimes(count + 1));
});

it('evicts full 5 MiB payloads to respect the 20 MiB cache storage budget', async () => {
  // Browsers use Blob URLs; jsdom's data-URI fallback repeatedly parses megabytes.
  const createDescriptor = Object.getOwnPropertyDescriptor(URL, 'createObjectURL');
  const revokeDescriptor = Object.getOwnPropertyDescriptor(URL, 'revokeObjectURL');
  const create = vi.fn(() => 'blob:cache-budget');
  const revoke = vi.fn();
  // Valid fixture only: avoid jsdom's slow codec; the bounded-payload test keeps real atob.
  const decode = vi.spyOn(globalThis, 'atob').mockImplementation((data) => Buffer.from(data, 'base64').toString('latin1'));
  const data = Buffer.alloc(5 * 1024 * 1024, 'x').toString('base64');
  const decoded = Buffer.from(data, 'base64').toString('latin1');
  const originalFrom = Uint8Array.from;
  // Avoid the VM's per-byte callback overhead for this exact valid fixture only.
  // Preserve all 5 MiB in the real Blob, and delegate every unrelated conversion.
  const bytes = vi.spyOn(Uint8Array, 'from').mockImplementation(((...args: Parameters<typeof Uint8Array.from>) => {
    if (args[0] === decoded && typeof args[1] === 'function') return Buffer.from(decoded, 'latin1');
    return Reflect.apply(originalFrom, Uint8Array, args);
  }) as typeof Uint8Array.from);
  Object.defineProperties(URL, {
    createObjectURL: { configurable: true, value: create },
    revokeObjectURL: { configurable: true, value: revoke },
  });
  try {
    native.invoke.mockResolvedValue([{ ...payload, data }]);
    for (let index = 0; index < 2; index += 1) {
      const view = render(<AttachmentPreview {...props} sessionId={'cache-bytes-' + index} />);
      try {
        fireEvent.click(screen.getByRole('button'));
        await waitFor(() => expect(screen.getByRole('dialog').querySelector('img')).toBeTruthy());
      } finally { view.unmount(); }
    }
    const view = render(<AttachmentPreview {...props} sessionId="cache-bytes-0" />);
    try {
      fireEvent.click(screen.getByRole('button'));
      await waitFor(() => expect(screen.getByRole('dialog').querySelector('img')).toBeTruthy());
      expect(native.invoke).toHaveBeenCalledTimes(3);
      expect(create).toHaveBeenCalledTimes(3);
      for (let call = 1; call <= 3; call += 1) {
        expect(create).toHaveBeenNthCalledWith(call, expect.objectContaining({ size: 5 * 1024 * 1024 }));
      }
    } finally { view.unmount(); }
    expect(revoke).toHaveBeenCalledTimes(3);
  } finally {
    bytes.mockRestore();
    decode.mockRestore();
    if (createDescriptor) Object.defineProperty(URL, 'createObjectURL', createDescriptor);
    else Reflect.deleteProperty(URL, 'createObjectURL');
    if (revokeDescriptor) Object.defineProperty(URL, 'revokeObjectURL', revokeDescriptor);
    else Reflect.deleteProperty(URL, 'revokeObjectURL');
  }
});

it('does not create a Blob URL for an IPC result arriving after unmount', async () => {
  let resolve!: (value: unknown) => void;
  native.invoke.mockReturnValue(new Promise((yes) => { resolve = yes; }));
  const create = vi.fn(() => 'blob:stale');
  vi.stubGlobal('URL', { createObjectURL: create, revokeObjectURL: vi.fn() });
  try {
    const view = render(<AttachmentPreview {...props} sessionId="unmounted" />);
    fireEvent.click(screen.getByRole('button'));
    view.unmount();
    await act(async () => resolve([payload]));
    expect(create).not.toHaveBeenCalled();
  } finally { vi.unstubAllGlobals(); }
});
