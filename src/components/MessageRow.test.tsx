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
});
