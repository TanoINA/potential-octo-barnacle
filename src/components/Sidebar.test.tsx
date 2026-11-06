import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Session } from '../types/agent';
import { Sidebar } from './Sidebar';

describe('Sidebar Pinned Sessions', () => {
  const dummySessions: Session[] = [
    {
      id: 'sess-1',
      title: 'Pinned chat Alpha',
      createdAt: 1000,
      updatedAt: 2000,
      messageCount: 5,
      pinned: true,
      pinnedAt: 2000,
    },
    {
      id: 'sess-2',
      title: 'Normal chat Beta',
      createdAt: 500,
      updatedAt: 1500,
      messageCount: 2,
      pinned: false,
    },
  ];

  it('renders pinned and recent sections separately', () => {
    const onTogglePinSession = vi.fn();
    render(
      <Sidebar
        sessions={dummySessions}
        currentSessionId="sess-2"
        onNewSession={vi.fn()}
        onSelectSession={vi.fn()}
        onDeleteSession={vi.fn()}
        onRenameSession={vi.fn()}
        onTogglePinSession={onTogglePinSession}
        onExportSession={vi.fn()}
        onOpenSettings={vi.fn()}
        workspaceCwd=""
        onSelectWorkspace={vi.fn()}
        effectiveConfig={null}
      />,
    );

    expect(screen.getByText(/Pinned \(1\)/i)).toBeTruthy();
    expect(screen.getByText(/Recent conversations \(1\)/i)).toBeTruthy();
    expect(screen.getByText('Pinned chat Alpha')).toBeTruthy();
    expect(screen.getByText('Normal chat Beta')).toBeTruthy();
  });

  it('toggles pin status when shift-clicking a session item', () => {
    const onTogglePinSession = vi.fn();
    const onSelectSession = vi.fn();

    render(
      <Sidebar
        sessions={dummySessions}
        currentSessionId="sess-2"
        onNewSession={vi.fn()}
        onSelectSession={onSelectSession}
        onDeleteSession={vi.fn()}
        onRenameSession={vi.fn()}
        onTogglePinSession={onTogglePinSession}
        onExportSession={vi.fn()}
        onOpenSettings={vi.fn()}
        workspaceCwd=""
        onSelectWorkspace={vi.fn()}
        effectiveConfig={null}
      />,
    );

    const normalChatBtn = screen.getByText('Normal chat Beta').closest('button');
    expect(normalChatBtn).toBeTruthy();

    // Normal click selects session
    fireEvent.click(normalChatBtn!);
    expect(onSelectSession).toHaveBeenCalledWith('sess-2');
    expect(onTogglePinSession).not.toHaveBeenCalled();

    // Shift click toggles pin
    fireEvent.click(normalChatBtn!, { shiftKey: true });
    expect(onTogglePinSession).toHaveBeenCalledWith('sess-2');
  });

  it('triggers deep search on query and displays highlighted snippet', async () => {
    const onDeepSearch = vi.fn().mockResolvedValue([
      {
        sessionId: 'sess-2',
        sessionTitle: 'Normal chat Beta',
        sessionUpdatedAt: 1500,
        snippet: '...found the <mark>database</mark> schema...',
        matchCount: 1,
        score: -5.2,
      },
    ]);

    render(
      <Sidebar
        sessions={dummySessions}
        currentSessionId="sess-1"
        onNewSession={vi.fn()}
        onSelectSession={vi.fn()}
        onDeleteSession={vi.fn()}
        onRenameSession={vi.fn()}
        onDeepSearch={onDeepSearch}
        onExportSession={vi.fn()}
        onOpenSettings={vi.fn()}
        workspaceCwd=""
        onSelectWorkspace={vi.fn()}
        effectiveConfig={null}
      />,
    );

    const searchInput = screen.getByPlaceholderText(/Search messages/i);
    fireEvent.change(searchInput, { target: { value: 'database' } });

    await waitFor(() => {
      expect(onDeepSearch).toHaveBeenCalledWith('database');
    });

    await waitFor(() => {
      expect(screen.getByText(/database/i)).toBeTruthy();
    });
  });
});

describe('Sidebar deep-search snippet safety', () => {
  const dummySessions: Session[] = [
    {
      id: 'sess-1',
      title: 'Pinned chat Alpha',
      createdAt: 1000,
      updatedAt: 2000,
      messageCount: 5,
      pinned: true,
      pinnedAt: 2000,
    },
    {
      id: 'sess-2',
      title: 'Normal chat Beta',
      createdAt: 500,
      updatedAt: 1500,
      messageCount: 2,
      pinned: false,
    },
  ];

  it('never injects stored HTML payloads from deep-search snippets', async () => {
    // The snippet comes from the SQLite FTS index over stored (untrusted)
    // message/artifact content, so anything other than the <mark> markers
    // must be treated as literal text.
    const onDeepSearch = vi.fn().mockResolvedValue([
      {
        sessionId: 'sess-2',
        sessionTitle: 'Normal chat Beta',
        sessionUpdatedAt: 1500,
        snippet:
          '...found the <mark>database</mark> <img src=x onerror="window.__xssFired = true"> <script>window.__xssFired = true<\/script> schema...',
        matchCount: 1,
        score: -5.2,
      },
    ]);

    const { container } = render(
      <Sidebar
        sessions={dummySessions}
        currentSessionId="sess-1"
        onNewSession={vi.fn()}
        onSelectSession={vi.fn()}
        onDeleteSession={vi.fn()}
        onRenameSession={vi.fn()}
        onDeepSearch={onDeepSearch}
        onExportSession={vi.fn()}
        onOpenSettings={vi.fn()}
        workspaceCwd=""
        onSelectWorkspace={vi.fn()}
        effectiveConfig={null}
      />,
    );

    const searchInput = screen.getByPlaceholderText(/Search messages/i);
    fireEvent.change(searchInput, { target: { value: 'database' } });

    await waitFor(() => {
      expect(onDeepSearch).toHaveBeenCalledWith('database');
    });
    await waitFor(() => {
      expect(screen.getByText(/database/i)).toBeTruthy();
    });

    // No element may be created from the untrusted snippet content.
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('script')).toBeNull();
    expect((window as unknown as { __xssFired?: boolean }).__xssFired).toBeUndefined();

    // The payload must be displayed as literal text.
    expect(container.textContent).toContain('<img src=x onerror');
    expect(container.textContent).toContain('<script>window.__xssFired = true</script>');

    // Legitimate <mark> highlights still work.
    expect(container.querySelector('mark')?.textContent).toBe('database');
  });
});
