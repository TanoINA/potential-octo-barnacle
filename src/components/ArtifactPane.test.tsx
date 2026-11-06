import { render, screen } from '@testing-library/react';
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
