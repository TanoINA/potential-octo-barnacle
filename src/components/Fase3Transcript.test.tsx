import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ChatMessage, CouncilTurn, ToolCallItem } from '../types/agent';
import { MessageRow } from './MessageRow';
import { ExecutionTrace } from './ExecutionTrace';
import { MarkdownRenderer } from './MarkdownRenderer';
import { FilesChangedCard } from './FilesChangedCard';
import { InteractiveQuestionCard } from './InteractiveQuestionCard';
import { CouncilView } from './CouncilView';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

describe('Fase 3 Transcript & Inline Widgets', () => {
  it('renders MessageRow assistant prose flat and user bubble with semantic token while keeping long-prompt collapse', () => {
    const assistantMsg: ChatMessage = {
      id: 'a-1',
      role: 'assistant',
      content: 'Plain flat assistant prose paragraph.',
      timestamp: 1700000000000,
    };
    const { container, rerender } = render(
      <MessageRow message={assistantMsg} onSelectArtifact={vi.fn()} />,
    );

    const proseWrapper = screen.getByText('Plain flat assistant prose paragraph.').closest('.select-text');
    expect(proseWrapper).toBeTruthy();
    expect(proseWrapper?.className).not.toContain('bg-[#161b22]');
    expect(proseWrapper?.className).not.toContain('border-[#30363d]');
    expect(proseWrapper?.className).toContain('text-[var(--ui-text-primary)]');

    const longPrompt = Array.from({ length: 10 }, (_, idx) => `Line ${idx + 1}`).join('\n') + ' ' + 'X'.repeat(300);
    const userMsg: ChatMessage = {
      id: 'u-1',
      role: 'user',
      content: longPrompt,
      timestamp: 1700000001000,
    };
    rerender(<MessageRow message={userMsg} onSelectArtifact={vi.fn()} />);

    const userBubble = container.querySelector('.max-h-36')?.parentElement;
    expect(userBubble?.className).toContain('bg-[var(--ui-user-bubble)]');
    expect(userBubble?.className).not.toContain('bg-[#1f6feb]');

    const toggle = screen.getByRole('button', { name: 'Show more' });
    fireEvent.click(toggle);
    expect(screen.getByRole('button', { name: 'Show less' })).toBeTruthy();
    expect(container.querySelector('.max-h-36')).toBeNull();
  });

  it('renders ExecutionTrace as a flat scaffold without boxed chrome while preserving scroll & failure indicators', () => {
    const failedTool: ToolCallItem = {
      id: 't-fail',
      name: 'write_file',
      args: { path: 'src/index.css' },
      status: 'error',
      result: 'Denied',
    };
    const okTool: ToolCallItem = {
      id: 't-ok',
      name: 'read_file',
      args: { path: 'README.md' },
      status: 'done',
      result: 'ok',
    };
    const { container } = render(<ExecutionTrace toolCalls={[okTool, failedTool]} />);

    const trace = screen.getByTestId('execution-trace');
    expect(trace.className).not.toContain('border-[#30363d]/70');
    expect(trace.className).toContain('text-[var(--conversation-scaffold-text)]');

    const failedBadge = screen.getByText('1 failed');
    expect(failedBadge.classList.contains('text-[#f85149]')).toBe(true);

    fireEvent.click(screen.getByRole('button', { name: '2 main actions 1 failed' }));
    const list = container.querySelector('.max-h-80');
    expect(list?.classList.contains('overflow-y-auto')).toBe(true);
    expect(list?.classList.contains('overscroll-contain')).toBe(true);
  });

  it('renders MarkdownRenderer elements with semantic variables and zero hardcoded text-white classes', () => {
    const md = [
      '# Heading One',
      '',
      '**Bold text** inside paragraph.',
      '',
      '| Col | Val |',
      '| --- | --- |',
      '| A | 1 |',
      '',
      '```ts',
      'const x = 1;',
      '```',
    ].join('\n');

    const { container } = render(<MarkdownRenderer content={md} />);
    expect(container.querySelector('h1')?.className).toContain('text-[var(--ui-text-primary)]');
    expect(container.querySelector('strong')?.className).toContain('text-[var(--ui-text-primary)]');
    expect(container.querySelector('[data-code-block="true"]')?.className).toContain('bg-[var(--ui-code-bg)]');
    expect(container.innerHTML).not.toContain('text-white');
  });

  it('uses semantic --ui-widget-bg surfaces across FilesChangedCard, InteractiveQuestionCard, and CouncilView', () => {
    const toolCalls: ToolCallItem[] = [{
      id: 'fc-1',
      name: 'edit_file',
      args: { Path: 'src/index.css' },
      status: 'done',
      filesChanged: [{ path: 'src/index.css', additions: 4, deletions: 1 }],
      artifactId: 'diff-1',
    }];
    const { container: fcContainer } = render(
      <FilesChangedCard toolCalls={toolCalls} onSelectArtifact={vi.fn()} />,
    );
    expect((fcContainer.firstElementChild as HTMLElement).className).toContain('bg-[var(--ui-widget-bg)]');
    expect(fcContainer.innerHTML).not.toContain('text-white');

    const { container: qContainer } = render(
      <InteractiveQuestionCard
        questions={[{ question: 'Select theme?', options: ['Nous', 'Graphite'] }]}
        onSelectOption={vi.fn()}
      />,
    );
    expect(screen.getByTestId('interactive-question-card').className).toContain('bg-[var(--ui-widget-bg)]');
    expect(qContainer.querySelectorAll('button')).toHaveLength(2);

    const councilTurn: CouncilTurn = {
      candidateA: { providerId: 'a', model: 'Model A', content: 'Option A', isStreaming: false },
      candidateB: { providerId: 'b', model: 'Model B', content: 'Option B', isStreaming: false },
    };
    const { container: cContainer } = render(
      <CouncilView councilTurn={councilTurn} onSelectWinner={vi.fn()} />,
    );
    expect(cContainer.innerHTML).toContain('bg-[var(--ui-widget-bg)]');
  });
});
