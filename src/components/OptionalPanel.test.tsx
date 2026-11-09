import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { lazy } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { OptionalPanel } from './OptionalPanel';

function deferredPanel() {
  let resolve!: (module: { default: () => React.ReactNode }) => void;
  const Panel = lazy(() => new Promise<{ default: () => React.ReactNode }>((done) => { resolve = done; }));
  return { Panel, resolve: (content: () => React.ReactNode) => resolve({ default: content }) };
}

describe('OptionalPanel', () => {
  it('shows an accessible loading dialog, then delivers the original action once', async () => {
    const { Panel, resolve } = deferredPanel();
    const onAction = vi.fn();
    const onClose = vi.fn();
    render(<OptionalPanel label="Settings" onClose={onClose}><Panel /></OptionalPanel>);
    const dialog = screen.getByRole('dialog', { name: 'Settings: loading' });
    expect(dialog.classList.contains('ui-panel-enter')).toBe(true);
    expect(dialog.parentElement!.classList.contains('ui-overlay-enter')).toBe(true);
    expect(dialog.parentElement!.classList.contains('ui-panel-enter')).toBe(false);
    expect(screen.getByRole('status').textContent).toContain('Loading Settings');
    // Clicking the loading content must not act like a backdrop click.
    fireEvent.click(screen.getByRole('status'));
    expect(onClose).not.toHaveBeenCalled();
    await act(async () => resolve(() => <button onClick={onAction}>Save settings</button>));
    fireEvent.click(await screen.findByRole('button', { name: 'Save settings' }));
    expect(onAction).toHaveBeenCalledOnce();
    expect(screen.queryByRole('status')).toBeNull();
  });

  it.each(['button', 'escape', 'backdrop'])('allows closing during loading using %s', async (method) => {
    const { Panel, resolve } = deferredPanel();
    const onClose = vi.fn();
    const view = render(<OptionalPanel label="Tools" onClose={onClose}><Panel /></OptionalPanel>);
    const close = screen.getByRole('button', { name: 'Close Tools' });
    if (method === 'button') fireEvent.click(close);
    if (method === 'escape') fireEvent.keyDown(document, { key: 'Escape' });
    if (method === 'backdrop') fireEvent.click(screen.getByRole('dialog').parentElement!);
    expect(onClose).toHaveBeenCalledOnce();
    view.unmount();
    await act(async () => resolve(() => <div>Late panel</div>));
    expect(screen.queryByText('Late panel')).toBeNull();
  });

  it('contains a rejected chunk without removing the chat', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const Panel = lazy(() => Promise.reject(new Error('chunk unavailable')));
    const onClose = vi.fn();
    render(<><div>Chat stays available</div><OptionalPanel label="History" onClose={onClose}><Panel /></OptionalPanel></>);
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('Could not load History'));
    expect(screen.getByText('Chat stays available')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Close History' }));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
