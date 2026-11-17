import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ChatCanvas } from './ChatCanvas';

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke, convertFileSrc: (path: string) => path }));
vi.mock('@tauri-apps/api/event', () => ({ listen: native.listen }));
vi.mock('./MessageRow', () => ({ MessageRow: () => null }));

type Props = ComponentProps<typeof ChatCanvas>;
const skills = [{ name: 'ponytail', description: 'Small changes', path: '', source: 'project' }];
const attachment = { id: 'draft-file', name: 'draft.ts', path: 'C:/work/draft.ts', isDir: false };
function setup(overrides: Partial<Props> = {}) {
  const props: Props = {
    messages: [], isStreaming: false, stopRequested: false, workspaceCwd: 'C:/work',
    currentSessionId: 'a', onSendMessage: vi.fn(async () => true), onStop: vi.fn(),
    onSelectArtifact: vi.fn(), onClearSession: vi.fn(), onNewSession: vi.fn(() => 'new'),
    onOpenSettings: vi.fn(), onOpenModelPicker: vi.fn(), effectiveConfig: null,
    sessionTitle: 'Draft test', artifactCount: 0, isArtifactOpen: false,
    onToggleArtifacts: vi.fn(), onListFiles: vi.fn(async () => []),
    onListSkills: vi.fn(async () => skills), ...overrides,
  };
  return { props, ...render(<ChatCanvas {...props} />) };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
beforeEach(() => {
  vi.clearAllMocks();
  native.invoke.mockResolvedValue([attachment]);
  HTMLElement.prototype.scrollIntoView = vi.fn();
});
afterEach(cleanup);

describe('per-session composer drafts', () => {
  it('restores text, attachments, and selected skills together', async () => {
    const { props, rerender } = setup();
    const input = screen.getByRole('textbox') as HTMLTextAreaElement;
    const file = new File(['draft'], 'draft.ts');
    Object.defineProperty(file, 'path', { value: attachment.path });
    fireEvent.drop(input, { dataTransfer: { files: [file] } });
    await screen.findByText('draft.ts');
    fireEvent.change(input, { target: { value: 'draft A /pony' } });
    fireEvent.click(await screen.findByRole('button', { name: /\/ponytail/ }));
    expect(screen.getByRole('button', { name: 'Remove skill ponytail' })).toBeTruthy();
    rerender(<ChatCanvas {...props} currentSessionId="b" />);
    expect(input.value).toBe('');
    expect(screen.queryByText('draft.ts')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Remove skill ponytail' })).toBeNull();
    fireEvent.change(input, { target: { value: 'draft B' } });
    rerender(<ChatCanvas {...props} isStreaming />);
    expect(input.value).toBe('draft A ');
    expect(screen.getByText('draft.ts')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Remove skill ponytail' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Preview skill ponytail' }));
    expect(await screen.findByText('Small changes')).toBeTruthy();
    rerender(<ChatCanvas {...props} currentSessionId="b" />);
    expect(input.value).toBe('draft B');
  });

  it('restores the draft cursor when returning to a session', async () => {
    const { props, rerender } = setup();
    const input = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: 'draft A' } });
    input.setSelectionRange(3, 3);
    fireEvent.select(input);
    rerender(<ChatCanvas {...props} currentSessionId="b" />);
    fireEvent.change(input, { target: { value: 'draft B' } });
    rerender(<ChatCanvas {...props} />);
    await waitFor(() => expect(input.selectionStart).toBe(3));
  });

  it('isolates drafts by workspace as well as session', () => {
    const { props, rerender } = setup();
    const input = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: 'workspace A' } });
    rerender(<ChatCanvas {...props} workspaceCwd="C:/other" />);
    expect(input.value).toBe('');
    fireEvent.change(input, { target: { value: 'workspace B' } });
    rerender(<ChatCanvas {...props} />);
    expect(input.value).toBe('workspace A');
  });

  it.each([true, false])('applies late send acceptance %s only to its original draft', async (accepted) => {
    const pending = deferred<boolean>();
    const { props, rerender } = setup({ onSendMessage: vi.fn(() => pending.promise) });
    const input = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: 'send A' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }));
    await waitFor(() => expect(props.onSendMessage).toHaveBeenCalledOnce());
    rerender(<ChatCanvas {...props} currentSessionId="b" />);
    fireEvent.change(input, { target: { value: 'keep B' } });
    await act(async () => pending.resolve(accepted));
    expect(input.value).toBe('keep B');
    rerender(<ChatCanvas {...props} />);
    expect(input.value).toBe(accepted ? '' : 'send A');
  });

  it('does not clear edits to the original draft while its send is pending', async () => {
    const pending = deferred<boolean>();
    const { props, rerender } = setup({ onSendMessage: vi.fn(() => pending.promise) });
    const input = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: 'send A' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }));
    await waitFor(() => expect(props.onSendMessage).toHaveBeenCalledOnce());
    rerender(<ChatCanvas {...props} currentSessionId="b" />);
    rerender(<ChatCanvas {...props} />);
    fireEvent.change(input, { target: { value: 'new A' } });
    await act(async () => pending.resolve(true));
    expect(input.value).toBe('new A');
  });
});
