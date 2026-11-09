import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import App from './App';

const mocks = vi.hoisted(() => {
  let resolve!: () => void;
  const ready = new Promise<void>((done) => { resolve = done; });
  return { ready, resolve: () => resolve(), newSession: vi.fn(), save: vi.fn(async () => undefined) };
});

vi.mock('./hooks/useAgent', () => ({ useAgent: () => ({
  artifacts: [], messages: [], sessions: [], settings: {},
  newSession: mocks.newSession, updateSettings: mocks.save,
}) }));
vi.mock('./hooks/useIsNarrowViewport', () => ({ useIsNarrowViewport: () => false }));
vi.mock('./components/Sidebar', () => ({ Sidebar: () => <aside>Sidebar ready</aside> }));
vi.mock('./components/ChatCanvas', () => ({ ChatCanvas: () => <main>Chat ready</main> }));
vi.mock('./components/ContextMenu', () => ({ ContextMenu: () => null }));
vi.mock('./components/SettingsModal', async () => {
  await mocks.ready;
  return { SettingsModal: ({ onSave, settings, onClose }: {
    onSave: (settings: object) => void; settings: object; onClose: () => void;
  }) => <div role="dialog" aria-modal="true" aria-label="Loaded settings">
    <button onClick={() => onSave(settings)}>Save loaded settings</button>
    <button onClick={onClose}>Close loaded settings</button>
  </div> };
});

afterEach(async () => {
  await act(async () => { cleanup(); });
});

it('preserves the chat during a cold import, guards shortcuts, and does not reopen a closed panel', async () => {
  render(<App />);
  expect(screen.getByText('Sidebar ready')).toBeTruthy();
  expect(screen.getByText('Chat ready')).toBeTruthy();
  fireEvent.keyDown(window, { key: ',', ctrlKey: true });
  const loading = await screen.findByRole('dialog', { name: 'Settings: loading' });
  fireEvent.click(screen.getByRole('status'));
  expect(screen.getByRole('dialog', { name: 'Settings: loading' })).toBe(loading);
  fireEvent.keyDown(window, { key: 'n', ctrlKey: true });
  fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
  expect(mocks.newSession).not.toHaveBeenCalled();
  expect(screen.getAllByRole('dialog')).toHaveLength(1);
  fireEvent.keyDown(document, { key: 'Escape' });
  expect(screen.queryByRole('dialog')).toBeNull();
  await act(async () => { mocks.resolve(); await mocks.ready; });
  expect(screen.queryByRole('dialog')).toBeNull();
  fireEvent.keyDown(window, { key: ',', ctrlKey: true });
  fireEvent.click(await screen.findByRole('button', { name: 'Save loaded settings' }));
  await waitFor(() => expect(mocks.save).toHaveBeenCalledOnce());
  fireEvent.click(screen.getByRole('button', { name: 'Close loaded settings' }));
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(screen.getByText('Chat ready')).toBeTruthy();
});
