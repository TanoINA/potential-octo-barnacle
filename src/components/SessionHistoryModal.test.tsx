import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ChatMessage } from '../types/agent';
import { SessionHistoryModal } from './SessionHistoryModal';

const messages: ChatMessage[] = [{
  id: 'user-1',
  role: 'user',
  content: 'Fix the parser',
  timestamp: 1,
}, {
  id: 'assistant-1',
  role: 'assistant',
  content: 'Implemented it',
  reasoning: 'Checked the boundary',
  timestamp: 2,
  toolCalls: [{
    id: 'turn_1_stream_0',
    name: 'edit_file',
    args: { path: 'src/main.ts' },
    status: 'done',
    result: 'Updated src/main.ts',
  }],
}];

function renderHistory(open: boolean, onClose = vi.fn()) {
  return render(<SessionHistoryModal
    open={open}
    onClose={onClose}
    sessionTitle="Parser work"
    sessionId="session-1"
    messages={messages}
    onCompact={vi.fn()}
    onExport={vi.fn()}
  />);
}

describe('SessionHistoryModal', () => {
  it('opens after a closed render without changing hook order', () => {
    const onClose = vi.fn();
    const { rerender } = renderHistory(false, onClose);
    expect(screen.queryByRole('dialog')).toBeNull();
    rerender(<SessionHistoryModal open onClose={onClose} sessionTitle="Parser work" sessionId="session-1" messages={messages} onCompact={vi.fn()} onExport={vi.fn()} />);
    expect(screen.getByRole('dialog')).toBeTruthy();
    expect(screen.getByText('src/main.ts')).toBeTruthy();
  });

  it('supports Escape dismissal', () => {
    const onClose = vi.fn();
    renderHistory(true, onClose);
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledOnce();
  });
});
