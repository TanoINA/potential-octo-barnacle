import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
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

function pendingSelection() { let resolve!: (value: boolean) => void; const promise = new Promise<boolean>(yes => { resolve=yes; }); return {promise,resolve}; }
it('ignores a late selection from a dismissed and reopened modal', async () => {
 const pending=pendingSelection(); const onClose=vi.fn(); const onSelectMessage=vi.fn(()=>pending.promise);
 const props={onClose,sessionTitle:'Parser',sessionId:'session-1',messages,onCompact:vi.fn(),onExport:vi.fn(),onSelectMessage};
 const {rerender}=render(<SessionHistoryModal {...props} open />);
 fireEvent.click(screen.getByRole('button',{name:'user: Fix the parser'}));
 rerender(<SessionHistoryModal {...props} open={false} />);
 rerender(<SessionHistoryModal {...props} open />);
 await act(async()=>{ pending.resolve(true); await pending.promise; });
 expect(onClose).not.toHaveBeenCalled();
});
it('keeps failed jumps open with a retryable error and closes only successful jumps', async () => {
 const onClose=vi.fn(); const select=vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
 render(<SessionHistoryModal open onClose={onClose} sessionTitle="Parser" sessionId="session-1" messages={messages} onSelectMessage={select} onSearch={async()=>[]} onCompact={vi.fn()} onExport={vi.fn()} />);
 await act(async()=>fireEvent.click(screen.getByRole('button',{name:'user: Fix the parser'})));
 expect(screen.getByRole('alert').textContent).toContain('Could not open'); expect(onClose).not.toHaveBeenCalled();
 await act(async()=>fireEvent.click(screen.getByRole('button',{name:'user: Fix the parser'})));
 expect(onClose).toHaveBeenCalledOnce();
});
it('discards stale search responses after a newer query', async () => {
 let resolve!: (value: any[])=>void; const pending=new Promise<any[]>(yes=>{resolve=yes;});
 const search=vi.fn().mockImplementationOnce(()=>pending).mockResolvedValue([]);
 render(<SessionHistoryModal open onClose={vi.fn()} sessionTitle="Parser" sessionId="session-1" messages={messages} onSearch={search} onCompact={vi.fn()} onExport={vi.fn()} />);
 const input=screen.getByRole('textbox',{name:'Search all conversations'});
 fireEvent.change(input,{target:{value:'first'}}); await waitFor(()=>expect(search).toHaveBeenCalledWith('first'));
 fireEvent.change(input,{target:{value:'second'}}); await waitFor(()=>expect(search).toHaveBeenCalledWith('second'));
 await act(async()=>{resolve([{sessionId:'old',sessionTitle:'Stale result',sessionUpdatedAt:1,snippet:'stale',matchCount:1,score:1}]); await pending;});
 expect(screen.queryByText('Stale result')).toBeNull();
});
