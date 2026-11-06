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
});
