import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import { MemoryModal } from './MemoryModal';

it('reloads and binds memory operations to the displayed workspace', async () => {
  const read = vi.fn(async (workspace?: string) => workspace === 'D:/B' ? 'B instructions' : 'A instructions');
  const save = vi.fn(async (_content: string, _workspace?: string) => undefined);
  const props = { open: true, onClose: vi.fn(), onReadMemory: read, onSaveMemory: save, onInitMemory: vi.fn(async () => 'created') };
  const view = render(<MemoryModal {...props} workspaceCwd="D:/A" />);
  const input = await screen.findByLabelText('AGENTS.md workspace instructions');
  await waitFor(() => expect((input as HTMLTextAreaElement).value).toBe('A instructions'));
  fireEvent.change(input, { target: { value: 'A draft' } });
  view.rerender(<MemoryModal {...props} workspaceCwd="D:/B" />);
  await waitFor(() => expect((screen.getByLabelText('AGENTS.md workspace instructions') as HTMLTextAreaElement).value).toBe('B instructions'));
  fireEvent.change(screen.getByLabelText('AGENTS.md workspace instructions'), { target: { value: 'B updated' } });
  fireEvent.click(screen.getByRole('button', { name: /save/i }));
  await waitFor(() => expect(save).toHaveBeenCalledWith('B updated', 'D:/B'));
  expect(save).not.toHaveBeenCalledWith('A draft', 'D:/B');
});
