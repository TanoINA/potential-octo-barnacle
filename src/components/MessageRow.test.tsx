import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChatMessage } from '../types/agent';
import { MessageRow } from './MessageRow';

describe('MessageRow Action Buttons', () => {
  const dummyUserMessage: ChatMessage = {
    id: 'msg-user-1',
    role: 'user',
    content: 'Tolong buatkan fungsi factorial di python',
    timestamp: 1700000000000,
  };

  const dummyAssistantMessage: ChatMessage = {
    id: 'msg-assistant-1',
    role: 'assistant',
    content: 'Tentu! Ini fungsi factorial di Python.',
    timestamp: 1700000005000,
  };

  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: {
        writeText: vi.fn().mockResolvedValue(undefined),
      },
    });
  });

  it('shows selected skill metadata separately from copied prompt text', () => {
    render(<MessageRow message={{ ...dummyUserMessage, selectedSkills: ['ponytail', 'systematic-debugging'] }} onSelectArtifact={vi.fn()} />);
    const skills = screen.getByLabelText('Skills used for this message');
    expect(skills.textContent).toContain('ponytail');
    expect(skills.textContent).toContain('systematic-debugging');
    fireEvent.click(screen.getByLabelText('Copy message'));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(dummyUserMessage.content);
  });

  it('renders a start failure even when the pre-created segments are empty', () => {
    render(<MessageRow message={{ ...dummyAssistantMessage, content: 'Error: Failed to start agent', segments: [{ id: 'seg_0', text: '', toolCalls: [] }] }} onSelectArtifact={vi.fn()} />);
    expect(screen.getByText('Error: Failed to start agent')).toBeTruthy();
  });

  it('renders copy button and copies content to clipboard', async () => {
    render(
      <MessageRow
        message={dummyUserMessage}
        onSelectArtifact={vi.fn()}
      />,
    );

    const copyBtn = screen.getByLabelText('Copy message');
    expect(copyBtn).toBeTruthy();

    fireEvent.click(copyBtn);

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
      'Tolong buatkan fungsi factorial di python',
    );

    await waitFor(() => {
      expect(screen.getByText('Copied')).toBeTruthy();
    });
  });

  it('allows user to enter inline edit mode and submit updated prompt', async () => {
    const onEditAndResend = vi.fn();
    render(
      <MessageRow
        message={dummyUserMessage}
        onSelectArtifact={vi.fn()}
        onEditAndResend={onEditAndResend}
      />,
    );

    const editBtn = screen.getByLabelText('Edit message');
    expect(editBtn).toBeTruthy();

    fireEvent.click(editBtn);

    // Textarea should now be visible with original content
    const textarea = screen.getByRole('textbox') as HTMLTextAreaElement;
    expect(textarea.value).toBe('Tolong buatkan fungsi factorial di python');

    // Update textarea content
    fireEvent.change(textarea, {
      target: { value: 'Tolong buatkan fungsi fibonacci di python' },
    });

    // Click Save & Submit
    const submitBtn = screen.getByRole('button', { name: /save & submit/i });
    fireEvent.click(submitBtn);

    expect(onEditAndResend).toHaveBeenCalledWith(
      'Tolong buatkan fungsi fibonacci di python',
    );
  });

  it('cancels edit mode without resending when Cancel is clicked', () => {
    const onEditAndResend = vi.fn();
    render(
      <MessageRow
        message={dummyUserMessage}
        onSelectArtifact={vi.fn()}
        onEditAndResend={onEditAndResend}
      />,
    );

    fireEvent.click(screen.getByLabelText('Edit message'));
    expect(screen.getByRole('textbox')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: /cancel/i }));

    expect(onEditAndResend).not.toHaveBeenCalled();
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it('calls onRetry when retry/regenerate button is clicked', () => {
    const onRetry = vi.fn();
    render(
      <MessageRow
        message={dummyAssistantMessage}
        onSelectArtifact={vi.fn()}
        onRetry={onRetry}
      />,
    );

    const retryBtn = screen.getByLabelText('Regenerate response');
    expect(retryBtn).toBeTruthy();

    fireEvent.click(retryBtn);
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it('calls onRollback when rollback button is clicked', () => {
    const onRollback = vi.fn();
    render(
      <MessageRow
        message={dummyAssistantMessage}
        onSelectArtifact={vi.fn()}
        onRollback={onRollback}
      />,
    );

    const rollbackBtn = screen.getByLabelText('Rollback to this message');
    expect(rollbackBtn).toBeTruthy();

    fireEvent.click(rollbackBtn);
    expect(onRollback).toHaveBeenCalledTimes(1);
  });

  it('renders preamble above execution trace and final content below execution trace', () => {
    const interleavedMessage: ChatMessage = {
      id: 'msg-interleaved-1',
      role: 'assistant',
      preamble: 'Siap, saya akan tes subagent dan task.',
      content: 'Semua pengujian berhasil diselesaikan.',
      timestamp: 1700000010000,
      toolCalls: [
        {
          id: 'call-1',
          name: 'run_command',
          args: { command: 'Get-ChildItem' },
          status: 'done',
        },
      ],
    };

    render(
      <MessageRow
        message={interleavedMessage}
        onSelectArtifact={vi.fn()}
      />,
    );

    const preambleEl = screen.getByTestId('message-preamble');
    expect(preambleEl).toBeTruthy();
    expect(preambleEl.textContent).toContain('Siap, saya akan tes subagent dan task.');

    const traceEl = screen.getByTestId('execution-trace');
    expect(traceEl).toBeTruthy();

    expect(screen.getByText('Semua pengujian berhasil diselesaikan.')).toBeTruthy();
  });

  it('renders multi-segment interleaved flow in chronological order', () => {
    const interleavedMessage: ChatMessage = {
      id: 'msg-interleaved-1',
      role: 'assistant',
      content: 'Langkah pertama... File ketemu... Semua selesai.',
      timestamp: 1700000020000,
      segments: [
        {
          id: 'seg-1',
          text: 'Langkah pertama: cek file dan siapkan reverse engineering.',
          toolCalls: [
            {
              id: 'call-1',
              name: 'run_command',
              args: { command: 'Get-ChildItem' },
              status: 'done',
              durationMs: 45,
            },
          ],
        },
        {
          id: 'seg-2',
          text: 'File ketemu: PE32 executable. Sekarang muat skema tool.',
          toolCalls: [
            {
              id: 'call-2',
              name: 'spawn_subagent',
              args: { role: 'Binary Analyzer', task: 'Analyze header' },
              status: 'done',
              durationMs: 120,
            },
          ],
        },
        {
          id: 'seg-3',
          text: 'Semua analisis selesai. Berikut kesimpulannya.',
          toolCalls: [],
        },
      ],
    };

    render(
      <MessageRow
        message={interleavedMessage}
        onSelectArtifact={vi.fn()}
      />,
    );

    expect(screen.getByText('Langkah pertama: cek file dan siapkan reverse engineering.')).toBeTruthy();
    expect(screen.getByText('File ketemu: PE32 executable. Sekarang muat skema tool.')).toBeTruthy();
    expect(screen.getByText('Semua analisis selesai. Berikut kesimpulannya.')).toBeTruthy();

    const traces = screen.getAllByTestId('execution-trace');
    expect(traces.length).toBe(2);
  });

  it('renders provider reconnecting notice indicator when notice is present on streaming message', () => {
    const streamingWithNotice: ChatMessage = {
      id: 'msg-stream-notice-1',
      role: 'assistant',
      content: '',
      timestamp: 1700000030000,
      isStreaming: true,
      notice: {
        message: 'No output from provider for 60s — reconnecting (attempt 2/3)...',
        attempt: 2,
        maxAttempts: 3,
      },
    };

    render(
      <MessageRow
        message={streamingWithNotice}
        onSelectArtifact={vi.fn()}
      />,
    );

    const noticeEl = screen.getByTestId('provider-notice');
    expect(noticeEl).toBeTruthy();
    expect(noticeEl.textContent).toContain('No output from provider for 60s — reconnecting (attempt 2/3)...');
    expect(screen.getByText('reconnecting')).toBeTruthy();
  });
});


const councilMessage: ChatMessage = {
  id: 'msg-council-1',
  role: 'assistant',
  content: 'Comparison complete',
  timestamp: 1700000040000,
  councilTurn: {
    candidateA: { providerId: 'a', model: 'Model A', content: 'Answer A', isStreaming: false },
    candidateB: { providerId: 'b', model: 'Model B', content: 'Answer B', isStreaming: false },
  },
};

it('gates council selection on its own message, not a later global run', () => {
  const onSelectCouncilWinner = vi.fn();
  const onSelectArtifact = vi.fn();
  const { rerender } = render(
    <MessageRow message={{ ...councilMessage, isStreaming: true }}
      onSelectArtifact={onSelectArtifact} onSelectCouncilWinner={onSelectCouncilWinner} />,
  );
  for (const button of screen.getAllByRole('button', { name: 'Use response' })) {
    expect((button as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(button);
  }
  expect(onSelectCouncilWinner).not.toHaveBeenCalled();

  rerender(
    <MessageRow message={{ ...councilMessage, isStreaming: false }} isStreaming={true}
      onSelectArtifact={onSelectArtifact} onSelectCouncilWinner={onSelectCouncilWinner} />,
  );
  for (const button of screen.getAllByRole('button', { name: 'Use response' })) {
    expect((button as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(button);
  }
  expect(onSelectCouncilWinner.mock.calls).toEqual([['msg-council-1', 'A'], ['msg-council-1', 'B']]);
});


it('hides council selection actions when no winner callback is provided', () => {
  render(<MessageRow message={councilMessage} onSelectArtifact={vi.fn()} />);
  expect(screen.getByText('Answer A')).toBeTruthy();
  expect(screen.getByText('Answer B')).toBeTruthy();
  expect(screen.queryAllByRole('button', { name: /Use response|Selected/ })).toHaveLength(0);
});

it('collapses long user prompts and expands on click', () => {
  const longPrompt = 'Line 1\nLine 2\nLine 3\nLine 4\nLine 5\nLine 6\nLine 7\nLine 8\nLine 9\nLine 10\n' + 'A'.repeat(300);
  const longMessage: ChatMessage = {
    id: 'msg-user-long',
    role: 'user',
    content: longPrompt,
    timestamp: 1700000000000,
  };

  const { rerender, container } = render(<MessageRow message={longMessage} onSelectArtifact={vi.fn()} />);

  // Should render Show more button
  const expandBtn = screen.getByRole('button', { name: 'Show more' });
  expect(expandBtn).toBeTruthy();
  expect(container.querySelector('.max-h-36')).toBeTruthy();

  // Click expand
  fireEvent.click(expandBtn);
  expect(screen.getByRole('button', { name: 'Show less' })).toBeTruthy();
  expect(container.querySelector('.max-h-36')).toBeNull();

  // Short message should not have Show more button
  const shortMessage: ChatMessage = {
    id: 'msg-user-short',
    role: 'user',
    content: 'Short prompt',
    timestamp: 1700000001000,
  };
  rerender(<MessageRow message={shortMessage} onSelectArtifact={vi.fn()} />);
  expect(screen.queryByRole('button', { name: 'Show more' })).toBeNull();
});

it('renders one clarification card when a tool call is mirrored into segments', () => {
  const question = { question: 'Font display?', options: ['Instrument Serif', 'Archivo 800'] };
  const tool = { id: 'turn_61_attempt_1_stream_0', name: 'ask_question', args: { questions: [question] }, status: 'done' as const };
  const message: ChatMessage = {
    id: 'msg-dup-question',
    role: 'assistant',
    content: '',
    timestamp: 1700000000000,
    toolCalls: [tool],
    segments: [
      { id: 'seg_0', text: '', toolCalls: [{ ...tool, id: 'turn_60_attempt_1_stream_0' }] },
      { id: 'seg_1', text: '', toolCalls: [tool] },
    ],
  };
  render(<MessageRow message={message} onSelectArtifact={vi.fn()} />);
  expect(screen.getAllByTestId('interactive-question-card')).toHaveLength(2);
});
