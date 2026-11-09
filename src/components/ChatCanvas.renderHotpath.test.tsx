import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ChatMessage } from '../types/agent';
import { MessageRow } from './MessageRow';
import { MarkdownRenderer } from './MarkdownRenderer';
import { ChatCanvas } from './ChatCanvas';

const counters = vi.hoisted(() => ({ parse: vi.fn(), row: vi.fn() }));
vi.mock('react-markdown', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-markdown')>();
  return { ...actual, default: (props: ComponentProps<typeof actual.default>) => {
    counters.parse(props.children);
    return actual.default(props);
  } };
});
vi.mock('./MessageRow', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./MessageRow')>();
  const { memo } = await import('react');
  return { ...actual, MessageRow: memo((props: ComponentProps<typeof actual.MessageRow>) => {
    counters.row(props.message.id);
    return <actual.MessageRow {...props} />;
  }) };
});
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue([]) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('@tanstack/react-virtual', () => {
  const virtualizer = {
    getVirtualItems: () => [
      { index: 0, key: 'user', start: 0 },
      { index: 1, key: 'assistant', start: 100 },
    ],
    getTotalSize: () => 200,
    measureElement: vi.fn(),
    scrollToIndex: vi.fn(),
  };
  return { useVirtualizer: () => virtualizer };
});

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
});
beforeEach(() => { counters.parse.mockClear(); counters.row.mockClear(); });

it('does not reparse ten static segments on status changes and reparses only the changed final segment', () => {
  const onSelectArtifact = vi.fn();
  const message: ChatMessage = {
    id: 'segmented', role: 'assistant', content: '', timestamp: 1700000000000, isStreaming: true,
    segments: Array.from({ length: 10 }, (_, index) => ({
      id: `segment-${index}`, text: `Static **segment ${index}**`,
      toolCalls: index === 9 ? [{ id: 'tool', name: 'run_command', args: { command: 'echo test' }, status: 'running' as const }] : [],
    })),
  };
  const { rerender } = render(<MessageRow message={message} onSelectArtifact={onSelectArtifact} />);
  expect(counters.parse).toHaveBeenCalledTimes(10);
  counters.parse.mockClear();
  const statusUpdate: ChatMessage = { ...message, segments: message.segments!.map((segment, index) => index === 9
    ? { ...segment, toolCalls: segment.toolCalls!.map((tool) => ({ ...tool, status: 'done' as const })) }
    : segment) };
  rerender(<MessageRow message={statusUpdate} onSelectArtifact={onSelectArtifact} />);
  expect(counters.parse).toHaveBeenCalledTimes(0);
  const complete = { ...statusUpdate, isStreaming: false };
  rerender(<MessageRow message={complete} onSelectArtifact={onSelectArtifact} />);
  expect(counters.parse).toHaveBeenCalledTimes(0);
  rerender(<MessageRow message={{ ...complete, segments: complete.segments!.map((segment, index) => index === 9
    ? { ...segment, text: 'Updated **final segment**' } : segment) }} onSelectArtifact={onSelectArtifact} />);
  expect(counters.parse).toHaveBeenCalledTimes(1);
  expect(counters.parse).toHaveBeenLastCalledWith('Updated **final segment**');
  expect(screen.getByText('final segment')).toBeTruthy();
});

describe('ChatCanvas virtual row action props', () => {
  it('skips static rows on composer updates while using replaced action callbacks with the correct row ID', () => {
    const messages: ChatMessage[] = [
      { id: 'user-1', role: 'user', content: 'Original prompt', timestamp: 1700000000000 },
      { id: 'assistant-1', role: 'assistant', content: 'Original answer', timestamp: 1700000001000 },
    ];
    const props: ComponentProps<typeof ChatCanvas> = {
      messages, isStreaming: false, stopRequested: false, onSendMessage: vi.fn(async () => true),
      onStop: vi.fn(), onSelectArtifact: vi.fn(), onClearSession: vi.fn(), onNewSession: () => 'new',
      onOpenSettings: vi.fn(), onOpenModelPicker: vi.fn(), effectiveConfig: null, sessionTitle: 'Render test',
      artifactCount: 0, isArtifactOpen: false, onToggleArtifacts: vi.fn(), onListFiles: async () => [],
      onRetryTurn: vi.fn(async () => true), onEditAndResendMessage: vi.fn(async () => true),
      onRollbackToMessage: vi.fn(async () => true),
    };
    const { rerender, container } = render(<ChatCanvas {...props} />);
    expect(counters.row.mock.calls.map(([id]) => id)).toEqual(expect.arrayContaining(['user-1', 'assistant-1']));
    counters.row.mockClear();
    fireEvent.change(screen.getByLabelText('Message AetherAgent'), { target: { value: 'Draft typing' } });
    expect(counters.row).toHaveBeenCalledTimes(0);
    const latest = {
      onRetryTurn: vi.fn(async () => true), onEditAndResendMessage: vi.fn(async () => true),
      onRollbackToMessage: vi.fn(async () => true),
    };
    rerender(<ChatCanvas {...props} {...latest} />);
    const user = within(container.querySelector('[data-message-id="user-1"]') as HTMLElement);
    const assistant = within(container.querySelector('[data-message-id="assistant-1"]') as HTMLElement);
    fireEvent.click(user.getByLabelText('Retry prompt'));
    fireEvent.click(assistant.getByLabelText('Regenerate response'));
    fireEvent.click(assistant.getByLabelText('Rollback to this message'));
    fireEvent.click(user.getByLabelText('Edit message'));
    fireEvent.change(user.getByRole('textbox'), { target: { value: '  Revised prompt  ' } });
    fireEvent.click(user.getByRole('button', { name: 'Save & Submit' }));
    expect(latest.onRetryTurn.mock.calls).toEqual([['user-1'], ['assistant-1']]);
    expect(latest.onRollbackToMessage).toHaveBeenCalledWith('assistant-1');
    expect(latest.onEditAndResendMessage).toHaveBeenCalledWith('user-1', 'Revised prompt');
    expect(props.onRetryTurn).not.toHaveBeenCalled();
    expect(props.onRollbackToMessage).not.toHaveBeenCalled();
    expect(props.onEditAndResendMessage).not.toHaveBeenCalled();
  });
});


it('keeps code-copy and interactive-plan local state across unchanged parent renders', async () => {
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: vi.fn().mockResolvedValue(undefined) } });
  const content = '```python\nif ready:\n    print("ok")\n```\n\n```plan\n- [ ] Verify behavior\n```';
  const { rerender } = render(<MarkdownRenderer content={content} />);
  expect(counters.parse).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByTitle('Copy code'));
  await waitFor(() => expect(screen.getByText('Copied')).toBeTruthy());
  expect(navigator.clipboard.writeText).toHaveBeenCalledWith('if ready:\n    print("ok")');
  fireEvent.click(screen.getByText('Verify behavior'));
  expect(screen.getByText('1/1 completed')).toBeTruthy();
  rerender(<MarkdownRenderer content={content} />);
  expect(counters.parse).toHaveBeenCalledTimes(1);
  expect(screen.getByText('Copied')).toBeTruthy();
  expect(screen.getByText('1/1 completed')).toBeTruthy();
});

it('can change a plan fence into a regular code fence without changing hook order', () => {
  const { rerender, container } = render(<MarkdownRenderer content={'```plan\n- [ ] Task\n```'} />);
  expect(screen.getByTestId('interactive-plan-card')).toBeTruthy();
  rerender(<MarkdownRenderer content={'```python\nprint("ok")\n```'} />);
  expect(screen.queryByTestId('interactive-plan-card')).toBeNull();
  expect(container.querySelector('pre code')?.textContent).toBe('print("ok")');
});
