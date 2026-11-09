import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ChatCanvas } from './ChatCanvas';

const mock = vi.hoisted(() => ({ options: null as any, scroll: vi.fn(), frames: [] as FrameRequestCallback[], instance: null as any, shift: 0, rowHeight: 100, observers: [] as any[], viewportHeight: 200, hideTarget: false }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('../hooks/useSpeechRecognition', () => ({ useSpeechRecognition: () => ({ isListening: false, toggleListening: vi.fn() }) }));
vi.mock('./MessageRow', () => ({ MessageRow: ({ message }: any) => <button>{message.content}</button> }));
vi.mock('./TimelineScrubber', () => ({ TimelineScrubber: ({ onJumpToMessage, activeMessageIndex }: any) => <><output data-testid="active-index">{activeMessageIndex}</output>{[0, 2, 5, 9].map((index) => <button key={index} onClick={() => onJumpToMessage(index)}>{index === 2 ? 'Timeline jump' : `Timeline ${index}`}</button>)}</> }));
vi.mock('@tanstack/react-virtual', () => ({ useVirtualizer: (options: any) => {
  mock.options = options;
  return mock.instance = {
    getVirtualItems: () => Array.from({ length: options.count }, (_, index) => ({ index, key: options.getItemKey(index), start: index * 100 })).filter((row) => !mock.hideTarget || row.index !== 5),
    getTotalSize: () => options.count * 100,
    measureElement: vi.fn(),
    scrollToIndex: (index: number, settings: any) => {
      mock.scroll(index, settings);
      const element = options.getScrollElement();
      if (element) element.scrollTop = settings.align === 'end' ? Math.max(0, (index + 1) * 100 - 200) : settings.align === 'center' ? index * 100 + 70 - 100 : index * 100;
    },
  };
} }));

const messages = (start = 10, count = 10) => Array.from({ length: count }, (_, i) => ({ id: `m${start + i}`, role: 'user' as const, content: `Message ${start + i}`, timestamp: 1700000000000 + i }));
function props(extra: Record<string, unknown> = {}): ComponentProps<typeof ChatCanvas> {
  return {
    messages: messages(), currentSessionId: 'a', sessionTitle: 'A', isStreaming: false, stopRequested: false,
    onSendMessage: vi.fn(async () => true), onStop: vi.fn(), onSelectArtifact: vi.fn(), onClearSession: vi.fn(),
    onNewSession: () => 'new', onOpenSettings: vi.fn(), onOpenModelPicker: vi.fn(), effectiveConfig: null,
    artifactCount: 0, isArtifactOpen: false, onToggleArtifacts: vi.fn(), onListFiles: async () => [], ...extra,
  };
}
const viewport = (container: HTMLElement) => container.querySelector('.overflow-y-auto') as HTMLElement;
const flushFrames = () => act(() => { const frames = mock.frames.splice(0); frames.forEach((frame) => frame(0)); });
const scroll = (element: HTMLElement, top: number) => { element.scrollTop = top; fireEvent.scroll(element); };
beforeEach(() => {
  // Shared setup's ResizeObserver is immutable; shadow it on a test window.
  vi.stubGlobal('window', Object.create(window, { ResizeObserver: { value: class {
    nodes = new Set<Element>();
    constructor(public callback: () => void) { mock.observers.push(this); }
    observe(node: Element) { this.nodes.add(node); }
    unobserve(node: Element) { this.nodes.delete(node); }
    disconnect() { this.nodes.clear(); }
  } } }));

  mock.scroll.mockClear(); mock.frames = []; mock.shift = 0; mock.rowHeight = 100; mock.observers = []; mock.viewportHeight = 200; mock.hideTarget = false;
  vi.useFakeTimers();

  vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { mock.frames.push(frame); return mock.frames.length; });
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const index = this.dataset.index;
    const parent = this.closest('.overflow-y-auto') as HTMLElement | null;
    const top = index === undefined ? 0 : Number(index) * 100 + mock.shift - (parent?.scrollTop ?? 0);
    return { top, bottom: top + (index === undefined ? mock.viewportHeight : mock.rowHeight), height: index === undefined ? mock.viewportHeight : mock.rowHeight, left: 0, right: 500, width: 500, x: 0, y: top, toJSON() {} };
  });
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => 200 });
  Object.defineProperty(HTMLElement.prototype, 'scrollHeight', { configurable: true, get: () => 3000 });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers(); });


it('centers measured geometry despite stale estimates and six late font reflows', () => {
  const { container } = render(<ChatCanvas {...props({ jumpToMessage: { messageId: 'm15', request: 1 } })} />);
  expect(mock.scroll).toHaveBeenLastCalledWith(5, { align: 'center' });
  mock.scroll.mockClear();
  mock.shift = 80; mock.rowHeight = 312;
  flushFrames(); flushFrames(); flushFrames();
  const target = container.querySelector('[data-message-id="m15"]')!;
  const centerError = () => {
    const row = target.getBoundingClientRect();
    const view = viewport(container).getBoundingClientRect();
    return Math.abs((row.top + row.bottom - view.top - view.bottom) / 2);
  };
  expect(centerError()).toBeLessThan(5);
  const observer = mock.observers.find((item) => item.nodes.has(target) && item.nodes.has(viewport(container)));
  expect(observer).toBeDefined();
  expect(observer.nodes.size).toBe(2);
  for (let i = 0; i < 6; i++) {
    mock.shift += 17; mock.rowHeight += 12;
    act(() => { observer.callback(); });
    expect(centerError()).toBeLessThan(5);
  }
  mock.viewportHeight = 280;
  act(() => { observer.callback(); });
  expect(centerError()).toBeLessThan(5);
  expect(mock.scroll).not.toHaveBeenCalled();
  act(() => { vi.advanceTimersByTime(800); });
  expect(observer.nodes.size).toBe(0);
  const top = viewport(container).scrollTop;
  mock.shift += 69;
  act(() => { observer.callback(); }); flushFrames();
  expect(viewport(container).scrollTop).toBe(top);
});

it('keeps timeline jumps start aligned and cancels pending search settling on user navigation', () => {
  render(<ChatCanvas {...props({ jumpToMessage: { messageId: 'm15', request: 1 } })} />);
  fireEvent.click(screen.getByRole('button', { name: 'Timeline jump' }));
  expect(mock.scroll).toHaveBeenLastCalledWith(2, { align: 'start' });
  mock.scroll.mockClear(); flushFrames(); flushFrames(); flushFrames();
  expect(mock.scroll).not.toHaveBeenCalled();
});

it('does not settle a stale search jump after session navigation', () => {
  const base = props({ jumpToMessage: { messageId: 'm15', request: 1 } });
  const { rerender } = render(<ChatCanvas {...base} />);
  rerender(<ChatCanvas {...base} currentSessionId="b" messages={messages(30)} jumpToMessage={null} />);
  mock.scroll.mockClear(); flushFrames(); flushFrames(); flushFrames();
  expect(mock.scroll).not.toHaveBeenCalled();
});

it('user scrolling cancels search settling without snapping back on subsequent frames', () => {
  const { container } = render(<ChatCanvas {...props({ jumpToMessage: { messageId: 'm15', request: 1 } })} />);
  const element = viewport(container);
  fireEvent.wheel(element, { deltaY: -30 }); scroll(element, 325);
  mock.scroll.mockClear(); flushFrames(); flushFrames(); flushFrames();
  expect(mock.scroll).not.toHaveBeenCalled();
  expect(element.scrollTop).toBe(325);
});

it.each(['wheel', 'pointerdown', 'touchstart', 'keydown'])('disconnects measured settling on %s input', (event) => {
  const { container, unmount } = render(<ChatCanvas {...props({ jumpToMessage: { messageId: 'm15', request: 1 } })} />);
  const element = viewport(container);
  const target = container.querySelector('[data-message-id="m15"]')!;
  const observer = mock.observers.find((item) => item.nodes.has(target) && item.nodes.has(element));
  expect(observer).toBeDefined();
  if (event === 'touchstart') fireEvent.touchStart(element, { touches: [{ clientY: 100 }] });
  else fireEvent(element, new Event(event, { bubbles: true }));
  expect(observer.nodes.size).toBe(0);
  const top = element.scrollTop;
  mock.shift += 69;
  act(() => { observer.callback(); }); flushFrames();
  expect(element.scrollTop).toBe(top);
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});

it('cleans observers and timers on unmount even before the first frame', () => {
  const { container, unmount } = render(<ChatCanvas {...props({ jumpToMessage: { messageId: 'm15', request: 1 } })} />);
  const element = viewport(container);
  const observer = mock.observers.find((item) => item.nodes.has(element));
  unmount();
  expect(observer.nodes.size).toBe(0);
  const top = element.scrollTop;
  mock.shift += 69;
  act(() => { observer.callback(); }); flushFrames();
  expect(element.scrollTop).toBe(top);
  expect(vi.getTimerCount()).toBe(0);
});

it('centers a target mounted by a later layout commit without another estimated jump', () => {
  const data = messages();
  const base = props({ messages: data, jumpToMessage: { messageId: 'm15', request: 1 } });
  mock.hideTarget = true;
  const { container, rerender } = render(<ChatCanvas {...base} />);
  flushFrames(); flushFrames(); flushFrames();
  const element = viewport(container);
  expect(container.querySelector('[data-message-id="m15"]')).toBeNull();
  expect(mock.frames).toHaveLength(0);
  mock.shift += 69;
  mock.scroll.mockClear();
  mock.hideTarget = false;
  rerender(<ChatCanvas {...base} sessionTitle="After measurement" />);
  const target = container.querySelector('[data-message-id="m15"]')!;
  const row = target.getBoundingClientRect();
  expect(Math.abs((row.top + row.bottom) / 2 - 100)).toBeLessThan(5);
  expect(mock.scroll).not.toHaveBeenCalled();
  expect(element.scrollTop).toBe(519);
});


it('corrects the 52px list origin for every timeline target and keeps only the latest rapid jump active', () => {
  mock.shift = 52;
  const { container, rerender } = render(<ChatCanvas {...props()} />);
  const element = viewport(container);
  const error = (index: number) => Math.abs(container.querySelector('[data-index="' + index + '"]')!.getBoundingClientRect().top - element.getBoundingClientRect().top);
  for (const index of [0, 2, 5, 9, 2, 5]) {
    fireEvent.click(screen.getByRole('button', { name: index === 2 ? 'Timeline jump' : 'Timeline ' + index }));
    expect(error(index)).toBeLessThanOrEqual(5);
    expect(screen.getByTestId('active-index').textContent).toBe(String(index));
  }
  flushFrames(); flushFrames(); flushFrames();
  expect(error(5)).toBeLessThanOrEqual(5);
  expect(mock.frames).toHaveLength(0);
  const target = container.querySelector('[data-index="5"]')!;
  const observer = mock.observers.find((item) => item.nodes.has(target) && item.nodes.has(element));
  expect(observer).toBeDefined();
  for (let i = 0; i < 6; i++) {
    mock.shift += 17;
    act(() => observer.callback());
    expect(error(5)).toBeLessThanOrEqual(5);
  }
  // The canvas, not scroll ratio, supplies the source index of the actual row.
  mock.shift = 52;
  scroll(element, 375);
  expect(screen.getByTestId('active-index').textContent).toBe('3');
  rerender(<ChatCanvas {...props({ currentSessionId: 'b', messages: messages(30) })} />);
  expect(observer.nodes.size).toBe(0);
  expect(vi.getTimerCount()).toBe(0);
});

it('start aligns a timeline target mounted after the three settling frames', () => {
  const base = props();
  mock.hideTarget = true;
  mock.shift = 52;
  const { container, rerender } = render(<ChatCanvas {...base} />);
  fireEvent.click(screen.getByRole('button', { name: 'Timeline 5' }));
  flushFrames(); flushFrames(); flushFrames();
  expect(mock.frames).toHaveLength(0);
  mock.scroll.mockClear();
  mock.hideTarget = false;
  rerender(<ChatCanvas {...base} sessionTitle="Mounted late" />);
  expect(container.querySelector('[data-index="5"]')!.getBoundingClientRect().top).toBeLessThanOrEqual(5);
  expect(viewport(container).scrollTop).toBe(552);
  expect(mock.scroll).not.toHaveBeenCalled();
});


it('lets a timeline jump supersede a pending prepend anchor without a later correction fighting it', async () => {
  const base = props({ hasOlderMessages: true, onLoadOlderMessages: vi.fn(async () => {}) });
  const { container, rerender } = render(<ChatCanvas {...base} />);
  const element = viewport(container);
  fireEvent.wheel(element, { deltaY: -30 }); scroll(element, 125);
  await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Load older messages' })));
  fireEvent.click(screen.getByRole('button', { name: 'Timeline 5' }));
  mock.scroll.mockClear();
  mock.shift = 52;
  rerender(<ChatCanvas {...base} messages={messages(0, 20)} />);
  expect(mock.scroll).not.toHaveBeenCalled();
  // A new jump uses the new source index, not the superseded prepend anchor.
  fireEvent.click(screen.getByRole('button', { name: 'Timeline jump' }));
  flushFrames(); flushFrames(); flushFrames();
  expect(container.querySelector('[data-index="2"]')!.getBoundingClientRect().top).toBe(0);
  expect(screen.getByTestId('active-index').textContent).toBe('2');
});
