import { act, fireEvent, render, screen } from '@testing-library/react';
import { lazy, useState } from 'react';
import { expect, it, vi } from 'vitest';
import { useModalAccessibility } from '../hooks/useModalAccessibility';
import { OptionalPanel } from './OptionalPanel';

it('traps focus across the lazy handoff and restores the original opener after close and unmount', async () => {
  const frames = new Map<number, FrameRequestCallback>();
  let nextFrame = 0;
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    frames.set(++nextFrame, callback);
    return nextFrame;
  });
  vi.stubGlobal('cancelAnimationFrame', (id: number) => frames.delete(id));
  vi.spyOn(HTMLElement.prototype, 'getClientRects').mockImplementation(function (this: HTMLElement) {
    return (this.isConnected ? [this.getBoundingClientRect()] : []) as unknown as DOMRectList;
  });
  const flushFrames = () => act(() => {
    const pending = [...frames.entries()];
    for (const [id, callback] of pending) {
      if (frames.delete(id)) callback(0);
    }
  });
  const tab = (shiftKey = false) => {
    // fireEvent does not emulate browser Tab navigation: cancellation plus
    // activeElement proves that the real hook performed the boundary wrap.
    expect(fireEvent.keyDown(document.activeElement!, { key: 'Tab', shiftKey, cancelable: true })).toBe(false);
  };

  const opener = document.createElement('button');
  opener.textContent = 'Open settings';
  document.body.append(opener);
  opener.focus();
  let resolve!: (module: { default: typeof LoadedModal }) => void;
  const Panel = lazy(() => new Promise<{ default: typeof LoadedModal }>((done) => { resolve = done; }));
  const onClose = vi.fn();
  function LoadedModal({ onClose }: { onClose: () => void }) {
    const ref = useModalAccessibility<HTMLDivElement>(true, onClose);
    return (
      <div ref={ref} role="dialog" aria-label="Loaded settings" tabIndex={-1}>
        <button type="button">First setting</button>
        <button type="button" onClick={onClose}>Close loaded settings</button>
      </div>
    );
  }
  function Harness() {
    const [open, setOpen] = useState(true);
    const close = () => { onClose(); setOpen(false); };
    return open ? <OptionalPanel label="Settings" onClose={close}><Panel onClose={close} /></OptionalPanel> : null;
  }

  const view = render(<Harness />);
  try {
    const fallback = screen.getByRole('dialog', { name: 'Settings: loading' });
    const fallbackClose = screen.getByRole('button', { name: 'Close Settings' });
    expect(document.activeElement).toBe(opener);
    flushFrames();
    expect(document.activeElement).toBe(fallbackClose);
    tab();
    expect(document.activeElement).toBe(fallbackClose);
    tab(true);
    expect(document.activeElement).toBe(fallbackClose);

    await act(async () => resolve({ default: LoadedModal }));
    expect(fallback.isConnected).toBe(false);
    expect(screen.queryByRole('dialog', { name: 'Settings: loading' })).toBeNull();
    flushFrames();
    const first = screen.getByRole('button', { name: 'First setting' });
    const last = screen.getByRole('button', { name: 'Close loaded settings' });
    expect(document.activeElement).toBe(first);
    tab(true);
    expect(document.activeElement).toBe(last);
    tab();
    expect(document.activeElement).toBe(first);

    fireEvent.click(last);
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(opener);
    view.unmount();
    flushFrames();
    expect(document.activeElement).toBe(opener);
    expect(frames.size).toBe(0);
  } finally {
    view.unmount();
    opener.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});
