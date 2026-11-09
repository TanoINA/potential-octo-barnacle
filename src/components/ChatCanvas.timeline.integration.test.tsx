import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ChatCanvas } from './ChatCanvas';

const mock = vi.hoisted(() => ({ options: null as any, scroll: vi.fn(), frames: [] as FrameRequestCallback[], instance: null as any, shift: 0, rowHeight: 100, observers: [] as any[], viewportHeight: 200, hideTarget: false }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('../hooks/useSpeechRecognition', () => ({ useSpeechRecognition: () => ({ isListening: false, toggleListening: vi.fn() }) }));
vi.mock('./MessageRow', () => ({ MessageRow: ({ message }: any) => <button>{message.content}</button> }));
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
    const top = index === undefined ? 0 : Number(index) * mock.rowHeight + mock.shift - (parent?.scrollTop ?? 0);
    return { top, bottom: top + (index === undefined ? mock.viewportHeight : mock.rowHeight), height: index === undefined ? mock.viewportHeight : mock.rowHeight, left: 0, right: 500, width: 500, x: 0, y: top, toJSON() {} };
  });
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => 200 });
  Object.defineProperty(HTMLElement.prototype, 'scrollHeight', { configurable: true, get: () => 3000 });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers(); });


it('keeps the real popover and track marker on the latest clicked turn, then derives the turn from visible assistant geometry', () => {
  mock.shift = 52;
  const data = messages().map((message, index) => ({ ...message, role: index % 2 ? 'assistant' as const : 'user' as const }));
  const { container } = render(<ChatCanvas {...props({ messages: data })} />);
  const element = viewport(container);
  const scrubber = screen.getByTestId('timeline-scrubber');
  fireEvent.mouseEnter(scrubber);
  const assertTurn = (turn: number) => {
    const current = scrubber.querySelectorAll('[aria-current="step"]');
    expect(current).toHaveLength(2);
    for (const button of current) expect(button.getAttribute('aria-label')).toContain('Jump to turn ' + turn + ':');
  };
  for (const [turn, index] of [[2, 2], [4, 6], [3, 4]]) {
    const label = 'Jump to turn ' + turn + ': Message ' + (10 + index);
    const buttons = screen.getAllByRole('button', { name: label });
    const button = buttons[buttons.length - 1];
    fireEvent.pointerDown(button);
    fireEvent.click(button);
    assertTurn(turn);
    expect(Math.abs(container.querySelector('[data-index="' + index + '"]')!.getBoundingClientRect().top)).toBeLessThanOrEqual(5);
  }
  flushFrames(); flushFrames(); flushFrames();
  assertTurn(3);
  const target = container.querySelector('[data-index="4"]')!;
  const observer = mock.observers.find((item) => item.nodes.has(target) && item.nodes.has(element));
  for (let i = 0; i < 6; i++) {
    mock.shift += 23;
    mock.rowHeight += 9;
    act(() => observer.callback());
    fireEvent.scroll(element);
    assertTurn(3);
    expect(Math.abs(target.getBoundingClientRect().top)).toBeLessThanOrEqual(5);
  }
  act(() => vi.advanceTimersByTime(800));
  expect(observer.nodes.size).toBe(0);
  mock.shift = 52; mock.rowHeight = 100;
  scroll(element, 575); // Assistant index 5 is first visible; user index 4 owns it.
  assertTurn(3);
  fireEvent.click(screen.getByRole('button', { name: 'Jump to top' }));
  expect(container.querySelector('[data-index="0"]')!.getBoundingClientRect().top).toBe(0);
  expect(element.scrollTop).toBe(52); // Header is allowed above the viewport.
  assertTurn(1);
});

it('clamps the real bottom arrow at the scroll end when the final assistant cannot start align', () => {
  mock.shift = 52;
  const data = messages().map((message, index) => ({ ...message, role: index % 2 ? 'assistant' as const : 'user' as const }));
  const { container } = render(<ChatCanvas {...props({ messages: data })} />);
  const element = viewport(container);
  let top = 0;
  Object.defineProperty(element, 'scrollHeight', { configurable: true, get: () => 1052 });
  Object.defineProperty(element, 'scrollTop', { configurable: true, get: () => top, set: (value: number) => { top = Math.max(0, Math.min(852, value)); } });
  fireEvent.click(screen.getByRole('button', { name: 'Jump to bottom' }));
  flushFrames(); flushFrames(); flushFrames();
  fireEvent.scroll(element);
  expect(top).toBe(852);
  const finalRow = container.querySelector('[data-index="9"]')!.getBoundingClientRect();
  expect(finalRow.bottom).toBe(element.getBoundingClientRect().bottom);
  expect(mock.scroll).toHaveBeenLastCalledWith(9, { align: 'start' });
  expect(screen.getByRole('button', { name: 'Jump to turn 5: Message 18' }).getAttribute('aria-current')).toBe('step');
});

it.each([
  ['down', 2, 2],
  ['up', 8, 5],
] as const)('refreshes both real timeline markers after scrolling %s when the visible row mounts without another scroll', (_direction, initialIndex, initialTurn) => {
  mock.rowHeight = 300;
  mock.hideTarget = true;
  const data = messages().map((message, index) => ({ ...message, role: index % 2 ? 'assistant' as const : 'user' as const }));
  const canvasProps = props({ messages: data });
  const { container, rerender } = render(<ChatCanvas {...canvasProps} />);
  const element = viewport(container);
  const scrubber = screen.getByTestId('timeline-scrubber');
  fireEvent.mouseEnter(scrubber);
  const assertTurn = (turn: number) => {
    const current = scrubber.querySelectorAll('[aria-current="step"]');
    expect(current).toHaveLength(2);
    for (const button of current) expect(button.getAttribute('aria-label')).toContain('Jump to turn ' + turn + ':');
  };
  scroll(element, initialIndex * 300 + 50);
  assertTurn(initialTurn);
  fireEvent.wheel(element, { deltaY: initialIndex < 5 ? 900 : -900 });
  scroll(element, 1550);
  expect(container.querySelector('[data-index="5"]')).toBeNull();
  // No old mounted row intersects: retain the prior marker until React commits.
  assertTurn(initialTurn);
  mock.hideTarget = false;
  rerender(<ChatCanvas {...canvasProps} />);
  expect(container.querySelector('[data-index="5"]')!.getBoundingClientRect().top).toBe(-50);
  assertTurn(3); // Assistant index 5 belongs to user index 4.
});

it('preserves the clicked marker while its target is unmounted across layout commits', () => {
  mock.hideTarget = true;
  const canvasProps = props();
  const { container, rerender } = render(<ChatCanvas {...canvasProps} />);
  const scrubber = screen.getByTestId('timeline-scrubber');
  fireEvent.mouseEnter(scrubber);
  const buttons = screen.getAllByRole('button', { name: 'Jump to turn 6: Message 15' });
  fireEvent.click(buttons[buttons.length - 1]);
  const assertTarget = () => {
    const current = scrubber.querySelectorAll('[aria-current="step"]');
    expect(current).toHaveLength(2);
    for (const button of current) expect(button.getAttribute('aria-label')).toContain('Jump to turn 6:');
  };
  expect(container.querySelector('[data-index="5"]')).toBeNull();
  assertTarget();
  rerender(<ChatCanvas {...canvasProps} />);
  assertTarget();
  mock.hideTarget = false;
  rerender(<ChatCanvas {...canvasProps} />);
  expect(container.querySelector('[data-index="5"]')!.getBoundingClientRect().top).toBe(0);
  assertTarget();
});