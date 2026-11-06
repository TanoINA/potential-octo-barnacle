import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { GlobalArtifactItem } from '../types/agent';
import { ArtifactsLibraryModal } from './ArtifactsLibraryModal';

describe('ArtifactsLibraryModal', () => {
  const dummyArtifacts: GlobalArtifactItem[] = [
    {
      sessionId: 'sess-alpha',
      sessionTitle: 'Alpha Session',
      sessionUpdatedAt: 1700000000000,
      artifact: {
        id: 'art-1',
        title: 'server.ts',
        type: 'code',
        language: 'typescript',
        content: 'const port = 3000;',
        filePath: 'src/server.ts',
        timestamp: 1700000000000,
      },
    },
    {
      sessionId: 'sess-beta',
      sessionTitle: 'Beta Session',
      sessionUpdatedAt: 1700000050000,
      artifact: {
        id: 'art-2',
        title: 'Notes.md',
        type: 'markdown',
        language: 'markdown',
        content: '# Implementation Plan\n\nStep 1 complete.',
        timestamp: 1700000050000,
      },
    },
  ];

  it('loads and displays artifacts list and preview', async () => {
    const onLoadArtifacts = vi.fn().mockResolvedValue(dummyArtifacts);
    const onSelectSession = vi.fn();
    const onClose = vi.fn();

    render(
      <ArtifactsLibraryModal
        open={true}
        onClose={onClose}
        onLoadArtifacts={onLoadArtifacts}
        onSelectSession={onSelectSession}
      />,
    );

    expect(onLoadArtifacts).toHaveBeenCalledTimes(1);

    await waitFor(() => {
      expect(screen.getAllByText('server.ts').length).toBeGreaterThanOrEqual(1);
      expect(screen.getByText('Notes.md')).toBeTruthy();
      expect(screen.getByText('2 artifacts')).toBeTruthy();
    });

    // Code snippet preview of default selected item (first one)
    expect(screen.getByText('const port = 3000;')).toBeTruthy();
  });

  it('filters by category chip', async () => {
    const onLoadArtifacts = vi.fn().mockResolvedValue(dummyArtifacts);

    render(
      <ArtifactsLibraryModal
        open={true}
        onClose={vi.fn()}
        onLoadArtifacts={onLoadArtifacts}
        onSelectSession={vi.fn()}
      />,
    );

    await waitFor(() => {
      expect(screen.getAllByText('server.ts').length).toBeGreaterThanOrEqual(1);
      expect(screen.getByText('Notes.md')).toBeTruthy();
    });

    // Click "Markdown" chip
    fireEvent.click(screen.getByRole('button', { name: 'Markdown' }));

    // Only Notes.md should remain visible in the results list
    expect(screen.queryByText('server.ts')).toBeNull();
    expect(screen.getAllByText('Notes.md').length).toBeGreaterThanOrEqual(1);
  });

  it('calls onSelectSession and closes when clicking Open chat', async () => {
    const onLoadArtifacts = vi.fn().mockResolvedValue(dummyArtifacts);
    const onSelectSession = vi.fn();
    const onClose = vi.fn();

    render(
      <ArtifactsLibraryModal
        open={true}
        onClose={onClose}
        onLoadArtifacts={onLoadArtifacts}
        onSelectSession={onSelectSession}
      />,
    );

    await waitFor(() => {
      expect(screen.getAllByText('server.ts').length).toBeGreaterThanOrEqual(1);
    });

    const openChatBtn = screen.getByRole('button', { name: /Open chat/i });
    fireEvent.click(openChatBtn);

    expect(onSelectSession).toHaveBeenCalledWith('sess-alpha');
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('surfaces a retryable error instead of claiming there are no artifacts when loading fails', async () => {
    const onLoadArtifacts = vi.fn().mockRejectedValue(new Error('index offline'));

    render(
      <ArtifactsLibraryModal
        open={true}
        onClose={vi.fn()}
        onLoadArtifacts={onLoadArtifacts}
        onSelectSession={vi.fn()}
      />,
    );

    // The empty-state copy must not be shown for a failed load.
    await waitFor(() => {
      expect(screen.getByText(/could not load artifacts/i)).toBeTruthy();
    });
    expect(screen.queryByText('No artifacts have been generated yet')).toBeNull();

    // Retry re-invokes the loader.
    fireEvent.click(screen.getByRole('button', { name: /retry/i }));
    await waitFor(() => expect(onLoadArtifacts).toHaveBeenCalledTimes(2));
  });
});
