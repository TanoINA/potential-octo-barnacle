import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { CouncilTurn } from '../types/agent';
import { CouncilView } from './CouncilView';

vi.mock('./MarkdownRenderer', () => ({ MarkdownRenderer: ({ content }: { content: string }) => <div>{content}</div> }));

function turn(streamingA: boolean, streamingB: boolean): CouncilTurn {
  return {
    candidateA: { providerId: 'a', model: 'Model A', content: 'Answer A', isStreaming: streamingA },
    candidateB: { providerId: 'b', model: 'Model B', content: 'Answer B', isStreaming: streamingB },
  };
}

describe('CouncilView winner selection', () => {
  it.each([[true, true], [false, true], [true, false]])(
    'disables both choices while either candidate streams (%s, %s)',
    (streamingA, streamingB) => {
      const onSelectWinner = vi.fn();
      const { rerender } = render(<CouncilView councilTurn={turn(streamingA, streamingB)} onSelectWinner={onSelectWinner} />);
      const buttons = screen.getAllByRole('button', { name: 'Use response' });
      for (const button of buttons) {
        expect((button as HTMLButtonElement).disabled).toBe(true);
        fireEvent.click(button);
      }
      expect(onSelectWinner).not.toHaveBeenCalled();

      rerender(<CouncilView councilTurn={turn(false, false)} onSelectWinner={onSelectWinner} />);
      for (const button of screen.getAllByRole('button', { name: 'Use response' })) {
        expect((button as HTMLButtonElement).disabled).toBe(false);
        fireEvent.click(button);
      }
      expect(onSelectWinner.mock.calls).toEqual([['A'], ['B']]);
    },
  );
});


it('keeps completed candidates disabled until the parent run finishes', () => {
  const onSelectWinner = vi.fn();
  const councilTurn = turn(false, false);
  const { rerender } = render(
    <CouncilView councilTurn={councilTurn} isStreaming={true} onSelectWinner={onSelectWinner} />,
  );
  for (const button of screen.getAllByRole('button', { name: 'Use response' })) {
    expect((button as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(button);
  }
  expect(onSelectWinner).not.toHaveBeenCalled();

  rerender(<CouncilView councilTurn={councilTurn} isStreaming={false} onSelectWinner={onSelectWinner} />);
  for (const button of screen.getAllByRole('button', { name: 'Use response' })) {
    expect((button as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(button);
  }
  expect(onSelectWinner.mock.calls).toEqual([['A'], ['B']]);
});
