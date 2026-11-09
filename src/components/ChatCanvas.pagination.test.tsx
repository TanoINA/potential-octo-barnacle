import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { ChatCanvas } from './ChatCanvas';

const mock = vi.hoisted(() => ({ options: null as any, scroll: vi.fn(), frames: [] as FrameRequestCallback[], instance: null as any, shift: 0 }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('../hooks/useSpeechRecognition', () => ({ useSpeechRecognition: () => ({ isListening: false, toggleListening: vi.fn() }) }));
vi.mock('./MessageRow', () => ({ MessageRow: ({ message }: any) => <button>{message.content}</button> }));
vi.mock('./TimelineScrubber', () => ({ TimelineScrubber: () => <div>Timeline</div> }));
vi.mock('@tanstack/react-virtual', () => ({ useVirtualizer: (options: any) => {
  mock.options = options;
  return mock.instance = {
    getVirtualItems: () => Array.from({ length: options.count }, (_, index) => ({ index, key: options.getItemKey(index), start: index * 100 })),
    getTotalSize: () => options.count * 100,
    measureElement: vi.fn(),
    scrollToIndex: (index: number, settings: any) => {
      mock.scroll(index, settings);
      const element = options.getScrollElement();
      if (element) element.scrollTop = settings.align === 'end' ? Math.max(0, (index + 1) * 100 - 200) : index * 100;
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
  mock.scroll.mockClear(); mock.frames = []; mock.shift = 0;
  vi.stubGlobal('requestAnimationFrame', (frame: FrameRequestCallback) => { mock.frames.push(frame); return mock.frames.length; });
  vi.stubGlobal('cancelAnimationFrame', vi.fn());
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const index = this.dataset.index;
    const parent = this.closest('.overflow-y-auto') as HTMLElement | null;
    const top = index === undefined ? 0 : Number(index) * 100 + mock.shift - (parent?.scrollTop ?? 0);
    return { top, bottom: top + (index === undefined ? 200 : 100), height: index === undefined ? 200 : 100, left: 0, right: 500, width: 500, x: 0, y: top, toJSON() {} };
  });
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => 200 });
  Object.defineProperty(HTMLElement.prototype, 'scrollHeight', { configurable: true, get: () => 3000 });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers(); });

it('offers accessible explicit history controls, total count, loading state and retry', async () => {
  const load = vi.fn(async () => {});
  const base = props({ hasOlderMessages: true, totalMessageCount: 30, onLoadOlderMessages: load });
  const { rerender } = render(<ChatCanvas {...base} />);
  expect(screen.getByText('30 messages')).toBeTruthy();
  expect(screen.getByText('10 / 30 messages loaded')).toBeTruthy();
  await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Load older messages' })));
  expect(load).toHaveBeenCalledTimes(1);
  rerender(<ChatCanvas {...base} loadingOlderMessages />);
  expect((screen.getByRole('button', { name: /Loading older messages/ }) as HTMLButtonElement).disabled).toBe(true);
  rerender(<ChatCanvas {...base} historyError="History unavailable" />);
  expect(screen.getByRole('alert').textContent).toContain('History unavailable');
  await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Retry loading older messages' })));
  expect(load).toHaveBeenCalledTimes(2);
});


it('preserves the visible message pixel offset on prepend without scrolling to last', async () => {
  const base = props({ hasOlderMessages: true, onLoadOlderMessages: vi.fn(async () => {}) });
  const { container, rerender } = render(<ChatCanvas {...base} />);
  flushFrames();
  const element = viewport(container);
  fireEvent.wheel(element, { deltaY: -40 }); scroll(element, 125);
  await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Load older messages' })));
  mock.scroll.mockClear();
  rerender(<ChatCanvas {...base} messages={messages(0, 20)} />); flushFrames();
  expect(mock.scroll).toHaveBeenCalledWith(11, { align: 'start' });
  expect(mock.scroll.mock.calls.some(([, options]) => options.align === 'end')).toBe(false);
  expect(element.scrollTop).toBe(1125);
});

it('loads once only for intentional upward scrolling near the top, never from spinner scroll events', async () => {
  let finish!: () => void;
  const load = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
  const base = props({ hasOlderMessages: true, onLoadOlderMessages: load });
  const { container, rerender } = render(<ChatCanvas {...base} />); flushFrames();
  const element = viewport(container);
  scroll(element, 100); expect(load).not.toHaveBeenCalled();
  fireEvent.wheel(element, { deltaY: -10 }); scroll(element, 90);
  expect(load).toHaveBeenCalledTimes(1);
  scroll(element, 80); scroll(element, 70); expect(load).toHaveBeenCalledTimes(1);
  await act(async () => finish());
  rerender(<ChatCanvas {...base} messages={messages(0, 20)} />); flushFrames();
  scroll(element, 20); expect(load).toHaveBeenCalledTimes(1);
});
it('hides stale conversation actions and disables composing during navigation', () => {
  render(<ChatCanvas {...props({ isSessionLoading: true })} />);
  expect(screen.getByRole('status').textContent).toBe('Loading conversation\u2026');
  expect(screen.queryByRole('button', { name: 'Message 10' })).toBeNull();
  expect((screen.getByRole('textbox') as HTMLTextAreaElement).disabled).toBe(true);
});
it('restores cached session message and pixel offset after navigation, without a rename reset', () => {
  const base = props(); const { container, rerender } = render(<ChatCanvas {...base} />); flushFrames();
  const element = viewport(container); fireEvent.wheel(element, { deltaY: -10 }); scroll(element, 325);
  mock.scroll.mockClear(); rerender(<ChatCanvas {...base} sessionTitle="Renamed" />); flushFrames();
  expect(element.scrollTop).toBe(325); expect(mock.scroll).not.toHaveBeenCalled();
  rerender(<ChatCanvas {...base} isSessionLoading />);
  rerender(<ChatCanvas {...base} currentSessionId="b" messages={messages(30)} />); flushFrames();
  expect(mock.scroll).toHaveBeenCalledWith(9, { align: 'end' });
  mock.scroll.mockClear(); rerender(<ChatCanvas {...base} />); flushFrames();
  expect(mock.scroll).toHaveBeenCalledWith(3, { align: 'start' }); expect(element.scrollTop).toBe(325);
});
it('restores the cached anchor when navigation clears preview before the destination finishes loading', () => {
  const base = props();
  const { container, rerender } = render(<ChatCanvas {...base} />);
  const element = viewport(container);
  fireEvent.wheel(element, { deltaY: -10 }); scroll(element, 325);
  const anchor = () => container.querySelector('[data-message-id="m13"]')!.getBoundingClientRect().top;
  expect(anchor()).toBe(-25);

  const leave = { messageId: '', request: 1 };
  rerender(<ChatCanvas {...base} isSessionLoading jumpToMessage={leave} />);
  rerender(<ChatCanvas {...base} currentSessionId="b" messages={messages(30)} isSessionLoading jumpToMessage={leave} />);
  rerender(<ChatCanvas {...base} currentSessionId="b" messages={messages(30)} jumpToMessage={leave} />);
  mock.scroll.mockClear();

  const back = { messageId: '', request: 2 };
  rerender(<ChatCanvas {...base} isSessionLoading jumpToMessage={back} />);
  rerender(<ChatCanvas {...base} jumpToMessage={back} />); flushFrames();
  expect(mock.scroll).toHaveBeenCalledWith(3, { align: 'start' });
  expect(mock.scroll.mock.calls.some(([, options]) => options.align === 'end')).toBe(false);
  expect(element.scrollTop).toBe(325);
  expect(anchor()).toBe(-25);
});

it('preserves the reading anchor when pending navigation is cancelled by selecting the current chat', () => {
  const base = props();
  const { container, rerender } = render(<ChatCanvas {...base} />);
  const element = viewport(container);
  fireEvent.wheel(element, { deltaY: -10 }); scroll(element, 325);
  rerender(<ChatCanvas {...base} isSessionLoading jumpToMessage={{ messageId: '', request: 1 }} />);
  mock.scroll.mockClear();
  rerender(<ChatCanvas {...base} jumpToMessage={{ messageId: '', request: 2 }} />);
  expect(element.scrollTop).toBe(325);
  expect(mock.scroll.mock.calls.some(([, options]) => options.align === 'end')).toBe(false);
});

it.each(['ArrowDown', 'PageDown', ' ', 'End'])('does not fight intentional %j keyboard scrolling after cache restoration', (key) => {
  const base = props();
  const { container, rerender } = render(<ChatCanvas {...base} />);
  const element = viewport(container);
  fireEvent.wheel(element, { deltaY: -10 }); scroll(element, 325);
  rerender(<ChatCanvas {...base} currentSessionId="b" messages={messages(30)} />);
  rerender(<ChatCanvas {...base} />);
  fireEvent.keyDown(element, { key });
  scroll(element, 450);
  rerender(<ChatCanvas {...base} sessionTitle="Keyboard scroll" />);
  expect(element.scrollTop).toBe(450);
});

it('bounds cached viewport measurement correction without a continuing animation loop', () => {
  vi.useFakeTimers();
  const base = props();
  const { container, rerender, unmount } = render(<ChatCanvas {...base} />);
  const element = viewport(container);
  fireEvent.wheel(element, { deltaY: -10 }); scroll(element, 325);
  rerender(<ChatCanvas {...base} currentSessionId="b" messages={messages(30)} />);
  rerender(<ChatCanvas {...base} />);
  flushFrames(); flushFrames(); flushFrames();
  expect(mock.frames).toHaveLength(0);
  mock.shift = 45;
  rerender(<ChatCanvas {...base} sessionTitle="Measured" />);
  expect(element.scrollTop).toBe(370);
  act(() => { vi.advanceTimersByTime(800); });
  mock.shift = 90;
  rerender(<ChatCanvas {...base} sessionTitle="After correction expires" />);
  expect(element.scrollTop).toBe(370);
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});

it('cancels restored viewport correction on intentional input and allows an explicit latest request', () => {
  vi.useFakeTimers();
  const base = props();
  const { container, rerender, unmount } = render(<ChatCanvas {...base} />);
  const element = viewport(container);
  fireEvent.wheel(element, { deltaY: -10 }); scroll(element, 325);
  rerender(<ChatCanvas {...base} currentSessionId="b" messages={messages(30)} />);
  const reset = { messageId: '', request: 11 };
  rerender(<ChatCanvas {...base} jumpToMessage={reset} />);
  expect(element.scrollTop).toBe(325);
  fireEvent.wheel(element, { deltaY: -10 }); scroll(element, 250);
  mock.shift = 45;
  rerender(<ChatCanvas {...base} jumpToMessage={reset} sessionTitle="After user scroll" />);
  expect(element.scrollTop).toBe(250);
  mock.scroll.mockClear();
  rerender(<ChatCanvas {...base} jumpToMessage={{ messageId: '', request: 12 }} />);
  expect(mock.scroll).toHaveBeenCalledWith(9, { align: 'end' });
  unmount();
  expect(vi.getTimerCount()).toBe(0);
});

it('ignores stale pagination completion from a previous session', async () => {
  let reject!: (error: Error) => void;
  const load = vi.fn(() => new Promise<void>((_, no) => { reject = no; }));
  const base = props({ hasOlderMessages: true, onLoadOlderMessages: load });
  const { rerender } = render(<ChatCanvas {...base} />);
  fireEvent.click(screen.getByRole('button', { name: 'Load older messages' }));
  rerender(<ChatCanvas {...base} currentSessionId="b" messages={messages(30)} />); flushFrames();
  mock.scroll.mockClear(); await act(async () => reject(new Error('stale'))); flushFrames();
  expect(mock.scroll).not.toHaveBeenCalled(); expect(screen.queryByRole('alert')).toBeNull();
  await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Load older messages' })));
  expect(load).toHaveBeenCalledTimes(2);
});

it('anchors late measured row growth above the visible row but never at bottom', async () => {
  const base = props({ hasOlderMessages: true, onLoadOlderMessages: vi.fn(async () => {}) });
  const { container, rerender } = render(<ChatCanvas {...base} />);
  const element = viewport(container); fireEvent.wheel(element, { deltaY: -20 }); scroll(element, 125);
  await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Load older messages' })));
  const prepended = messages(0, 20); rerender(<ChatCanvas {...base} messages={prepended} />);
  expect(mock.instance.shouldAdjustScrollPositionOnItemSizeChange({ index: 10 })).toBe(true);
  expect(mock.instance.shouldAdjustScrollPositionOnItemSizeChange({ index: 11 })).toBe(false);
  mock.shift = 45; rerender(<ChatCanvas {...base} messages={prepended} sessionTitle="Renamed" />);
  expect(element.scrollTop).toBe(1170);
  fireEvent.wheel(element, { deltaY: -20 }); scroll(element, 1000);
  mock.shift = 60; rerender(<ChatCanvas {...base} messages={prepended} sessionTitle="Renamed again" />);
  expect(element.scrollTop).toBe(1000);
  scroll(element, 2800);
  expect(mock.instance.shouldAdjustScrollPositionOnItemSizeChange({ index: 0 })).toBe(false);
});
it('preserves delayed prepend measurements after the cached-navigation settle window', async () => {
  vi.useFakeTimers();
  const base = props({ hasOlderMessages: true, onLoadOlderMessages: vi.fn(async () => {}) });
  const { container, rerender } = render(<ChatCanvas {...base} />);
  const element = viewport(container);
  fireEvent.wheel(element, { deltaY: -20 }); scroll(element, 125);
  await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Load older messages' })));
  const prepended = messages(0, 20);
  rerender(<ChatCanvas {...base} messages={prepended} />);
  expect(element.scrollTop).toBe(1125);
  act(() => vi.advanceTimersByTime(1000));
  mock.shift = 137;
  rerender(<ChatCanvas {...base} messages={prepended} sessionTitle="Delayed measured row" />);
  expect(element.scrollTop).toBe(1262);
});

it('bounds viewport restoration to the six most recently visited sessions', () => {
  const base = props(); const { container, rerender } = render(<ChatCanvas {...base} />);
  fireEvent.wheel(viewport(container), { deltaY: -10 }); scroll(viewport(container), 325);
  for (let i = 0; i < 8; i++) rerender(<ChatCanvas {...base} currentSessionId={'session-' + i} />);
  mock.scroll.mockClear(); rerender(<ChatCanvas {...base} />);
  expect(mock.scroll).toHaveBeenCalledWith(9, { align: 'end' });
});
it('keeps a bottom viewport at bottom when navigating back to cached messages', () => {
  const base = props(); const { rerender } = render(<ChatCanvas {...base} />);
  rerender(<ChatCanvas {...base} currentSessionId="b" messages={messages(30)} />);
  mock.scroll.mockClear(); rerender(<ChatCanvas {...base} />);
  expect(mock.scroll).toHaveBeenCalledWith(9, { align: 'end' });
});

it('releases a prepend anchor when the user sends and resumes following the bottom', async () => {
  const base = props({ hasOlderMessages: true, onLoadOlderMessages: vi.fn(async () => {}) });
  const { container, rerender } = render(<ChatCanvas {...base} />);
  const element = viewport(container); fireEvent.wheel(element, { deltaY: -20 }); scroll(element, 125);
  await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Load older messages' })));
  rerender(<ChatCanvas {...base} messages={messages(0, 20)} />);
  fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Next prompt' } });
  await act(async () => fireEvent.click(screen.getByRole('button', { name: 'Send message' })));
  rerender(<ChatCanvas {...base} messages={messages(0, 21)} />);
  expect(element.scrollTop).toBe(1900);
});

it('outlines only the explicit jump target and offers latest while browsing ordinary older history', () => {
 const {container}=render(<ChatCanvas {...props({jumpToMessage:{messageId:'m12',request:1}})} />); flushFrames();
 expect(container.querySelector('[data-message-id="m12"]')?.className).toContain('outline-2');
 const element=viewport(container); fireEvent.wheel(element,{deltaY:-30}); scroll(element,100);
 expect(screen.getByRole('button',{name:'Return to latest'})).toBeTruthy();
});

it('clears the explicit outline shortly after the jump without changing message data', () => {
 vi.useFakeTimers(); const data=messages();
 const {container}=render(<ChatCanvas {...props({messages:data,jumpToMessage:{messageId:'m12',request:1}})} />);
 expect(container.querySelector('[data-message-id="m12"]')?.className).toContain('outline-2');
 act(()=>{vi.advanceTimersByTime(2000);});
 expect(container.querySelector('[data-message-id="m12"]')?.className).not.toContain('outline-2');
 vi.useRealTimers();
});


it('floating preview return loads canonical latest and never pretends the preview tail is latest while pending', async () => {
  let finish!: () => void;
  const latest = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
  const base = props({ isHistoryPreview: true, onLoadLatestMessages: latest });
  const { container, rerender } = render(<ChatCanvas {...base} />); flushFrames();
  const element = viewport(container);
  fireEvent.wheel(element, { deltaY: -30 }); scroll(element, 325);
  const floating = screen.getAllByRole('button', { name: 'Return to latest' }).find((button) => button.classList.contains('absolute'))!;
  expect(floating).toBeTruthy();
  mock.scroll.mockClear();
  fireEvent.click(floating);
  expect(latest).toHaveBeenCalledTimes(1);
  expect(mock.scroll).not.toHaveBeenCalled();
  expect(element.scrollTop).toBe(325);
  expect((screen.getByRole('textbox') as HTMLTextAreaElement).disabled).toBe(true);
  expect(screen.getAllByRole('button', { name: 'Return to latest' }).includes(floating)).toBe(true);
  rerender(<ChatCanvas {...base} sessionTitle="Still pending" />); flushFrames();
  expect(mock.scroll).not.toHaveBeenCalled();
  await act(async () => finish());
  rerender(<ChatCanvas {...base} isHistoryPreview={false} messages={messages(100)} jumpToMessage={{ messageId: '', request: 2 }} />);
  expect(mock.scroll).toHaveBeenLastCalledWith(9, { align: 'end' });
  expect((screen.getByRole('textbox') as HTMLTextAreaElement).disabled).toBe(false);
});

it('ordinary older-history floating return scrolls the loaded canonical tail without fetching latest', () => {
  const latest = vi.fn(async () => {});
  const { container } = render(<ChatCanvas {...props({ onLoadLatestMessages: latest })} />); flushFrames();
  const element = viewport(container); fireEvent.wheel(element, { deltaY: -30 }); scroll(element, 325);
  mock.scroll.mockClear(); fireEvent.click(screen.getByRole('button', { name: 'Return to latest' }));
  expect(latest).not.toHaveBeenCalled();
  expect(mock.scroll).toHaveBeenLastCalledWith(9, { align: 'end' });
});

it('preview return without a latest callback does not falsely scroll or enable composing', () => {
  const { container } = render(<ChatCanvas {...props({ isHistoryPreview: true })} />); flushFrames();
  const element = viewport(container); fireEvent.wheel(element, { deltaY: -30 }); scroll(element, 325);
  mock.scroll.mockClear(); fireEvent.click(screen.getByRole('button', { name: 'Return to latest' })); flushFrames();
  expect(mock.scroll).not.toHaveBeenCalled();
  expect(element.scrollTop).toBe(325);
  expect((screen.getByRole('textbox') as HTMLTextAreaElement).disabled).toBe(true);
});
