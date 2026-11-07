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

  it('renders multi-segment Hermes style interleaved flow in chronological order', () => {
    const hermesMessage: ChatMessage = {
      id: 'msg-hermes-1',
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
        message={hermesMessage}
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
