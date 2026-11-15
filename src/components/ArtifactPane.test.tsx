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

  it('toggles between Preview and Raw views for code and markdown artifacts', () => {
    const markdownArtifact: Artifact = {
      id: 'doc-1',
      title: 'Plan doc',
      type: 'markdown',
      content: '# Heading\n- item 1\n- item 2',
      timestamp: 2,
    };
    const props = { onSelectArtifact: vi.fn(), onClose: vi.fn() };
    const { container } = render(<ArtifactPane {...props} artifacts={[markdownArtifact]} activeArtifactId="doc-1" initialTab="code" />);

    const previewBtn = screen.getByRole('button', { name: 'Preview' });
    const rawBtn = screen.getByRole('button', { name: 'Raw' });

    // Default is preview mode, rendering markdown
    expect(previewBtn).toBeTruthy();
    expect(rawBtn).toBeTruthy();
    expect(screen.getByText('Heading')).toBeTruthy();
    expect(previewBtn.className).toContain('text-[#58a6ff]');

    // Click Raw to switch to raw lines view
    fireEvent.click(rawBtn);
    expect(rawBtn.className).toContain('text-[#58a6ff]');
    expect(screen.queryByText('Heading')).toBeNull();
    expect(container.querySelector('.font-mono')).toBeTruthy();

    // Click Preview to switch back
    fireEvent.click(previewBtn);
    expect(screen.getByText('Heading')).toBeTruthy();
    expect(previewBtn.className).toContain('text-[#58a6ff]');
  });

  it('renders sandboxed iframe for HTML artifact preview and raw code for raw mode', () => {
    const htmlArtifact: Artifact = {
      id: 'html-1',
      title: 'Interactive Dashboard',
      type: 'code',
      language: 'html',
      content: '<h1>Live Dashboard</h1><button>Click me</button>',
      timestamp: 3,
    };
    const props = { onSelectArtifact: vi.fn(), onClose: vi.fn() };
    const { container } = render(<ArtifactPane {...props} artifacts={[htmlArtifact]} activeArtifactId="html-1" initialTab="code" />);

    // In preview mode, an iframe is rendered with srcDoc
    const iframe = container.querySelector('iframe');
    expect(iframe).toBeTruthy();
    expect(iframe?.getAttribute('srcDoc')).toContain('Live Dashboard');

    // Switch to raw mode, raw lines are rendered instead of iframe
    fireEvent.click(screen.getByRole('button', { name: 'Raw' }));
    expect(container.querySelector('iframe')).toBeNull();
    expect(container.querySelector('.font-mono')).toBeTruthy();
  });
});
