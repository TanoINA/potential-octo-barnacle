import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Artifact } from '../types/agent';
import { ArtifactPane } from './ArtifactPane';

const artifact: Artifact = {
  id: 'terminal-1',
  title: 'Terminal: test',
  type: 'terminal',
  content: 'first\n',
  timestamp: 1,
};

describe('ArtifactPane', () => {
  it.each(['code', 'diff', 'terminal'] as const)('preserves a user-selected Live Shell draft when a new %s artifact arrives', (type) => {
    const props = { onSelectArtifact: vi.fn(), onClose: vi.fn() };
    const { rerender } = render(<ArtifactPane {...props} artifacts={[artifact]} activeArtifactId={artifact.id} />);
    fireEvent.click(screen.getByRole('button', { name: 'Live Shell' }));
    const input = screen.getByPlaceholderText('Type command and press Enter...') as HTMLInputElement;
    fireEvent.change(input, { target: { value: 'git status --short' } });

    const incoming: Artifact = { ...artifact, id: 'new-artifact', type, title: 'New artifact' };
    rerender(<ArtifactPane {...props} artifacts={[artifact, incoming]} activeArtifactId={incoming.id} />);

    expect(screen.queryByPlaceholderText('Type command and press Enter...')).toBe(input);
    expect(input.value).toBe('git status --short');
    // The new artifact is still available when the user explicitly leaves Shell.
    fireEvent.click(screen.getByRole('button', { name: type === 'code' ? 'Code Viewer' : type === 'diff' ? 'Diff Viewer' : 'Tool Output' }));
    expect(screen.queryByPlaceholderText('Type command and press Enter...')).toBeNull();
    expect(screen.getByRole('combobox', { name: 'Select artifact' })).toBeTruthy();
  });

  it('does not override an explicit shell tab on mount or terminal updates', () => {
    const props = {
      activeArtifactId: artifact.id,
      onSelectArtifact: vi.fn(),
      onClose: vi.fn(),
      initialTab: 'shell' as const,
    };
    const { rerender } = render(<ArtifactPane {...props} artifacts={[artifact]} />);
    expect(screen.getByPlaceholderText('Type command and press Enter...')).toBeTruthy();
    rerender(<ArtifactPane {...props} artifacts={[{ ...artifact, content: 'first\nsecond\n' }]} />);
    expect(screen.getByPlaceholderText('Type command and press Enter...')).toBeTruthy();
  });
});
