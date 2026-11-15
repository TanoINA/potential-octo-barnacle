import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ChatMessage } from '../types/agent';
import { TimelineScrubber } from './TimelineScrubber';

describe('TimelineScrubber', () => {
  const dummyMessages: ChatMessage[] = [
    {
      id: 'msg-1',
      role: 'user',
      content: 'lu bisa install Everything Claude Code?',
      timestamp: 1697612580000, // Oct 18, 2023 2:03 PM
    },
    {
      id: 'msg-2',
      role: 'assistant',
      content: 'Tentu, saya bisa install.',
      timestamp: 1697612600000,
    },
    {
      id: 'msg-3',
      role: 'user',
      content: 'everything claude code ada apa aja fiturnya?',
      timestamp: 1697624160000, // Oct 18, 2023 5:16 PM
    },
    {
      id: 'msg-4',
      role: 'assistant',
      content: 'Berikut fitur-fiturnya...',
      timestamp: 1697624200000,
    },
    {
      id: 'msg-5',
      role: 'user',
      content: 'nah gwa mau install https://github.com/test',
      timestamp: 1697642640000, // Oct 18, 2023 10:24 PM
    },
  ];

  it('renders nothing when there is only 1 user turn', () => {
    const singleTurnMessages: ChatMessage[] = [
      dummyMessages[0],
      dummyMessages[1],
    ];

    const { container } = render(
      <TimelineScrubber
        messages={singleTurnMessages}
        sessionTitle="Single Turn Session"
        scrollElement={null}
        onJumpToMessage={vi.fn()}
      />,
    );

    expect(container.firstChild).toBeNull();
  });

  it('renders vertical scrubber track when there are multiple user turns', () => {
    render(
      <TimelineScrubber
        messages={dummyMessages}
        sessionTitle="Installing coding tools and agent toolkits"
        scrollElement={null}
        onJumpToMessage={vi.fn()}
      />,
    );

    const scrubber = screen.getByTestId('timeline-scrubber');
    expect(scrubber).toBeTruthy();
  });

  it('shows floating hover popover with session title and turns on mouse enter', () => {
    render(
      <TimelineScrubber
        messages={dummyMessages}
        sessionTitle="Installing coding tools and agent toolkits"
        scrollElement={null}
        onJumpToMessage={vi.fn()}
      />,
    );

    const scrubber = screen.getByTestId('timeline-scrubber');
    fireEvent.mouseEnter(scrubber);

    const popover = screen.getByTestId('timeline-popover');
    expect(popover).toBeTruthy();
    expect(screen.getByText('Installing coding tools and agent toolkits')).toBeTruthy();
    expect(screen.getByText('3 turns')).toBeTruthy();
    expect(screen.getByText(/lu bisa install Everything Claude Code/)).toBeTruthy();
    expect(screen.getByText(/everything claude code ada apa aja fiturnya/)).toBeTruthy();
    expect(screen.getByText(/nah gwa mau install https:\/\/github.com\/test/)).toBeTruthy();
  });

  it('calls onJumpToMessage when clicking a turn item in the popover', () => {
    const onJumpToMessage = vi.fn();
    render(
      <TimelineScrubber
        messages={dummyMessages}
        sessionTitle="Installing coding tools and agent toolkits"
        scrollElement={null}
        onJumpToMessage={onJumpToMessage}
      />,
    );

    const scrubber = screen.getByTestId('timeline-scrubber');
    fireEvent.mouseEnter(scrubber);

    const secondTurnSnippet = screen.getByText(/everything claude code ada apa aja fiturnya/);
    fireEvent.click(secondTurnSnippet);

    // Turn 2 is at index 2 in dummyMessages
    expect(onJumpToMessage).toHaveBeenCalledWith(2);
  });

  it('jumps to top and bottom when clicking scrubber chevron arrows', () => {
    const onJumpToMessage = vi.fn();
    render(
      <TimelineScrubber
        messages={dummyMessages}
        sessionTitle="Installing coding tools and agent toolkits"
        scrollElement={null}
        onJumpToMessage={onJumpToMessage}
      />,
    );

    const topBtn = screen.getByLabelText('Jump to top');
    fireEvent.click(topBtn);
    expect(onJumpToMessage).toHaveBeenCalledWith(0);

    const bottomBtn = screen.getByLabelText('Jump to bottom');
    fireEvent.click(bottomBtn);
    expect(onJumpToMessage).toHaveBeenCalledWith(dummyMessages.length - 1);
  });

  it('uses the source message anchor rather than physical scroll percentage and follows prepended history', () => {
    const scrollElement = document.createElement('div');
    Object.defineProperties(scrollElement, {
      scrollTop: { value: 900, writable: true },
      scrollHeight: { value: 1100 },
      clientHeight: { value: 100 },
    });
    const onJumpToMessage = vi.fn();
    const props = { messages: dummyMessages, sessionTitle: 'Unequal turns', scrollElement, onJumpToMessage };
    const { rerender } = render(<TimelineScrubber {...props} activeMessageIndex={3} />);
    fireEvent.mouseEnter(screen.getByTestId('timeline-scrubber'));
    const selected = () => screen.getByTestId('timeline-popover').querySelector('[class~="bg-[#21262d]"]');
    expect(selected()?.textContent).toContain(dummyMessages[2].content);
    const secondRow = screen.getByText(dummyMessages[2].content).parentElement;
    expect(scrollElement.scrollTop).toBe(900);

    rerender(<TimelineScrubber {...props} activeMessageIndex={4} />);
    expect(selected()?.textContent).toContain(dummyMessages[4].content);
    rerender(<TimelineScrubber {...props} activeMessageIndex={-1} />);
    expect(selected()?.textContent).toContain(dummyMessages[0].content);
    rerender(<TimelineScrubber {...props} activeMessageIndex={999} />);
    expect(selected()?.textContent).toContain(dummyMessages[4].content);

    const older: ChatMessage[] = [
      { id: 'older-assistant', role: 'assistant', content: 'Earlier response', timestamp: 1 },
      { id: 'older-user', role: 'user', content: 'Earlier prompt', timestamp: 2 },
      ...dummyMessages,
    ];
    rerender(<TimelineScrubber {...props} messages={older} activeMessageIndex={5} />);
    expect(selected()?.textContent).toContain(dummyMessages[2].content);
    expect(screen.getByText(dummyMessages[2].content).parentElement).toBe(secondRow);
    fireEvent.click(screen.getByText(dummyMessages[4].content));
    expect(onJumpToMessage).toHaveBeenCalledExactlyOnceWith(6);
    // The parent owns click anchoring; the child must not invent optimistic state.
    expect(selected()?.textContent).toContain(dummyMessages[2].content);
  });

  it('tracks the first intersecting mounted native ID in both scroll directions, not overscan or DOM indices', () => {
    const scrollElement = document.createElement('div');
    Object.defineProperties(scrollElement, {
      scrollTop: { value: 900, writable: true },
      scrollHeight: { value: 1100 },
      clientHeight: { value: 100 },
    });
    const bounds = (top: number, bottom: number) => ({ top, bottom, left: 0, right: 100, width: 100, height: bottom - top, x: 0, y: top, toJSON: () => ({}) });
    vi.spyOn(scrollElement, 'getBoundingClientRect').mockReturnValue(bounds(100, 200));
    const rows = [dummyMessages[0], dummyMessages[3], dummyMessages[4]].map((message, index) => {
      const row = document.createElement('div');
      row.dataset.index = String(1000 + index); // Native IDs, not stale virtual offsets, locate source messages.
      row.dataset.messageId = message.id;
      scrollElement.append(row);
      return row;
    });
    let positions = [[0, 100], [80, 170], [170, 230]];
    rows.forEach((row, index) => vi.spyOn(row, 'getBoundingClientRect').mockImplementation(() => bounds(...positions[index] as [number, number])));
    const outside = document.createElement('div');
    outside.dataset.index = '0';
    outside.dataset.messageId = dummyMessages[0].id;
    document.body.append(outside);
    vi.spyOn(outside, 'getBoundingClientRect').mockReturnValue(bounds(100, 150));
    const removeListener = vi.spyOn(scrollElement, 'removeEventListener');
    const props = { messages: dummyMessages, sessionTitle: 'Mounted rows', scrollElement, onJumpToMessage: vi.fn() };
    try {
      const { rerender, unmount } = render(<TimelineScrubber {...props} />);
      fireEvent.mouseEnter(screen.getByTestId('timeline-scrubber'));
      const selected = () => screen.getByTestId('timeline-popover').querySelector('[class~="bg-[#21262d]"]');
      expect(selected()?.textContent).toContain(dummyMessages[2].content);
      positions = [[0, 50], [50, 100], [100, 200]];
      scrollElement.scrollTop = 950;
      fireEvent.scroll(scrollElement);
      expect(selected()?.textContent).toContain(dummyMessages[4].content);
      positions = [[90, 140], [140, 190], [190, 240]];
      scrollElement.scrollTop = 100;
      fireEvent.scroll(scrollElement);
      expect(selected()?.textContent).toContain(dummyMessages[0].content);
      rerender(<TimelineScrubber {...props} activeMessageIndex={4} />);
      fireEvent.scroll(scrollElement);
      expect(selected()?.textContent).toContain(dummyMessages[4].content);
      expect(screen.getByTestId('timeline-scrubber').querySelector('.bg-white')?.getAttribute('style')).toContain('10%');
      unmount();
      expect(removeListener).toHaveBeenCalledWith('scroll', expect.any(Function));
    } finally {
      outside.remove();
    }
  });

  it('provides stable button ticks and rows with one jump each and keeps background jumps approximate', () => {
    const onJumpToMessage = vi.fn();
    render(<TimelineScrubber messages={dummyMessages} sessionTitle="Buttons" scrollElement={null} activeMessageIndex={2} onJumpToMessage={onJumpToMessage} />);
    const root = screen.getByTestId('timeline-scrubber');
    fireEvent.mouseEnter(root);
    const buttons = screen.getAllByRole('button', { name: `Jump to turn 2: ${dummyMessages[2].content}` });
    expect(buttons).toHaveLength(2);
    for (const button of buttons) {
      expect(button.getAttribute('type')).toBe('button');
      expect(button.getAttribute('aria-current')).toBe('step');
      fireEvent.click(button);
      expect(onJumpToMessage).toHaveBeenCalledExactlyOnceWith(2);
      onJumpToMessage.mockClear();
    }
    for (const button of screen.getAllByRole('button', { name: `Jump to turn 1: ${dummyMessages[0].content}` })) {
      expect(button.hasAttribute('aria-current')).toBe(false);
    }
    expect(root.querySelector('[class*="scale-"]')).toBeNull();
    const track = root.querySelector('[title="Session timeline scrubber"]') as HTMLElement;
    vi.spyOn(track, 'getBoundingClientRect').mockReturnValue({ top: 100, height: 200 } as DOMRect);
    fireEvent.click(track, { clientY: 200 });
    expect(onJumpToMessage).toHaveBeenCalledExactlyOnceWith(2);
  });

  it('opens on focus, retains internal focus, closes on Escape and cleans delayed hiding on unmount', () => {
    vi.useFakeTimers();
    const scheduleHide = vi.spyOn(window, 'setTimeout');
    const clearHide = vi.spyOn(window, 'clearTimeout');
    try {
      const { unmount } = render(<TimelineScrubber messages={dummyMessages} sessionTitle="Focus" scrollElement={null} onJumpToMessage={vi.fn()} />);
      const root = screen.getByTestId('timeline-scrubber');
      const top = screen.getByRole('button', { name: 'Jump to top' });
      fireEvent.focus(top);
      expect(screen.queryByTestId('timeline-popover')).not.toBeNull();
      const row = screen.getByTestId('timeline-popover').querySelector('button')!;
      fireEvent.blur(top, { relatedTarget: row });
      fireEvent.focus(row, { relatedTarget: top });
      fireEvent.mouseLeave(root);
      fireEvent.keyDown(row, { key: 'Escape' });
      expect(screen.queryByTestId('timeline-popover')).toBeNull();
      expect(document.activeElement).toBe(top);
      fireEvent.focus(top);
      scheduleHide.mockClear();
      fireEvent.blur(top, { relatedTarget: document.body });
      expect(scheduleHide).toHaveBeenCalledExactlyOnceWith(expect.any(Function), 250);
      const firstTimer = scheduleHide.mock.results[0].value;
      fireEvent.focus(top);
      expect(clearHide).toHaveBeenCalledWith(firstTimer);
      scheduleHide.mockClear();
      fireEvent.mouseLeave(root);
      // A focused timeline must stay open even if the pointer leaves.
      expect(scheduleHide).not.toHaveBeenCalled();
      fireEvent.blur(top, { relatedTarget: document.body });
      expect(scheduleHide).toHaveBeenCalledExactlyOnceWith(expect.any(Function), 250);
      const finalTimer = scheduleHide.mock.results[0].value;
      unmount();
      expect(clearHide).toHaveBeenCalledWith(finalTimer);
    } finally {
      vi.useRealTimers();
    }
  });

  it('synchronizes the active turn to the final turn when scrolled to bottom even with unequal heights', () => {
    const scrollElement = document.createElement('div');
    Object.defineProperties(scrollElement, {
      scrollTop: { value: 1000, writable: true },
      scrollHeight: { value: 1100 },
      clientHeight: { value: 100 },
    });
    // Max scroll is 1000, scrollTop is 1000 => scrollRatio = 1.0 (at the bottom)
    // activeMessageIndex is 1 (assistant message of turn 1, because turn 2 is short and turn 1 top is still visible)
    render(
      <TimelineScrubber
        messages={dummyMessages}
        sessionTitle="Unequal Heights At Bottom"
        scrollElement={scrollElement}
        activeMessageIndex={1}
        onJumpToMessage={vi.fn()}
      />,
    );
    fireEvent.mouseEnter(screen.getByTestId('timeline-scrubber'));
    const selected = screen.getByTestId('timeline-popover').querySelector('[class~="bg-[#21262d]"]');
    // Must select the last user turn (dummyMessages[4]), not turn 1 (dummyMessages[0])
    expect(selected?.textContent).toContain(dummyMessages[4].content);
  });
});
