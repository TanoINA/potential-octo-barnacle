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

  it('toggles pin status when clicking the quick pin hover button', () => {
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

    const quickPin = screen.getByRole('button', { name: 'Pin Normal chat Beta to top' });
    fireEvent.click(quickPin);
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
    expect(searchInput.closest('label')?.getAttribute('aria-busy')).toBe('true');
    expect(searchInput.closest('aside')?.innerHTML).not.toMatch(/animate-|shimmer|glow/);

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

describe('Sidebar Git status & actions', () => {
  it('renders git branch and changed files count, toggles drawer, and submits commit', async () => {
    const onCommitWorkspace = vi.fn().mockResolvedValue('abc1234');
    const onRefreshGitStatus = vi.fn().mockResolvedValue(undefined);

    render(
      <Sidebar
        sessions={[]}
        currentSessionId=""
        onNewSession={vi.fn()}
        onSelectSession={vi.fn()}
        onDeleteSession={vi.fn()}
        onRenameSession={vi.fn()}
        onExportSession={vi.fn()}
        onOpenSettings={vi.fn()}
        workspaceCwd="D:/my-project"
        onSelectWorkspace={vi.fn()}
        effectiveConfig={null}
        gitStatus={{
          isRepo: true,
          branch: 'main',
          hasChanges: true,
          summary: '3 modified entries',
          files: [
            { path: 'src/App.tsx', status: 'M' },
            { path: 'README.md', status: 'M' },
            { path: 'newfile.ts', status: '??' },
          ],
        }}
        onRefreshGitStatus={onRefreshGitStatus}
        onCommitWorkspace={onCommitWorkspace}
      />,
    );

    expect(screen.getByText('main')).toBeTruthy();
    expect(screen.getByText('3 changes')).toBeTruthy();

    // Click the git toggle button to open changes drawer
    const gitToggleBtn = screen.getByLabelText('Toggle git details');
    fireEvent.click(gitToggleBtn);

    // Should display files in drawer
    expect(screen.getByText('src/App.tsx')).toBeTruthy();
    expect(screen.getByText('newfile.ts')).toBeTruthy();

    // Type commit message and click commit button
    const commitInput = screen.getByPlaceholderText(/Commit message/i);
    fireEvent.change(commitInput, { target: { value: 'feat: add git drawer' } });

    const commitBtn = screen.getByRole('button', { name: /commit/i });
    fireEvent.click(commitBtn);

    await waitFor(() => {
      expect(onCommitWorkspace).toHaveBeenCalledWith('feat: add git drawer');
    });
  });
});


const navigationProps = () => ({
  sessions: [{ id: 'history', title: 'History', createdAt: 1, updatedAt: 2, messageCount: 1 }],
  currentSessionId: 'history',
  onNewSession: vi.fn(), onSelectSession: vi.fn(), onDeleteSession: vi.fn(),
  onRenameSession: vi.fn(), onTogglePinSession: vi.fn(), onExportSession: vi.fn(),
  onOpenSettings: vi.fn(), onOpenArtifactsLibrary: vi.fn(),
  workspaceCwd: 'D:/project', onSelectWorkspace: vi.fn(), onClearWorkspace: vi.fn(),
  effectiveConfig: null,
});

describe('Sidebar chat activity', () => {
  it('shows activity for each chat independently of which chat is open', () => {
    const props = navigationProps();
    const sessions = ['working', 'saved', 'stopped', 'broken', 'untouched'].map((id) => ({
      id, title: id, createdAt: 1, updatedAt: 1, messageCount: 1,
    }));
    const { rerender } = render(<Sidebar {...props} sessions={sessions} currentSessionId="untouched"
      sessionActivities={{ working: 'running', saved: 'completed', stopped: 'cancelled', broken: 'failed' }} />);
    expect(screen.getByLabelText('Chat status: Running').closest('button')?.textContent).toContain('working');
    expect(screen.getByLabelText('Chat status: Completed').closest('button')?.textContent).toContain('saved');
    expect(screen.getByLabelText('Chat status: Stopped').closest('button')?.textContent).toContain('stopped');
    expect(screen.getByLabelText('Chat status: Failed').closest('button')?.textContent).toContain('broken');
    expect(screen.getAllByLabelText(/^Chat status:/)).toHaveLength(4);
    rerender(<Sidebar {...props} sessions={sessions} currentSessionId="saved"
      sessionActivities={{ working: 'saving', saved: 'running' }} />);
    expect(screen.getByLabelText('Chat status: Saving').closest('button')?.textContent).toContain('working');
    expect(screen.getByLabelText('Chat status: Running').closest('button')?.textContent).toContain('saved');
    expect(screen.queryByLabelText('Chat status: Completed')).toBeNull();
    expect(screen.queryByLabelText('Chat status: Failed')).toBeNull();
  });
});

describe('Sidebar calm navigation', () => {
  it('limits entrance motion to overlay mode and exposes the selected conversation', () => {
    const props = navigationProps();
    const { container, rerender } = render(<Sidebar {...props} />);
    expect(container.querySelector('aside')?.classList.contains('ui-sidebar-enter')).toBe(false);
    expect(screen.getByRole('button', { name: 'History' }).getAttribute('aria-current')).toBe('page');
    const row = screen.getByRole('button', { name: 'History' }).closest('.group');
    expect(row?.className).toContain('rounded-lg');
    expect(row?.className).not.toMatch(/58a6ff|1f6feb/);
    expect(container.innerHTML).not.toMatch(/animate-|shimmer|glow/);
    rerender(<Sidebar {...props} overlay />);
    expect(container.querySelector('aside')?.classList.contains('ui-sidebar-enter')).toBe(true);
    rerender(<Sidebar {...props} />);
    expect(container.querySelector('aside')?.classList.contains('ui-sidebar-enter')).toBe(false);
  });

  it('preserves rename Enter, Escape, IME handling and menu Escape', () => {
    const props = navigationProps();
    render(<Sidebar {...props} />);
    const openRename = () => {
      fireEvent.click(screen.getByRole('button', { name: 'Actions for History' }));
      fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
      return screen.getByRole('textbox', { name: 'Session title' });
    };
    const title = openRename();
    fireEvent.change(title, { target: { value: '  Renamed history  ' } });
    fireEvent.keyDown(title, { key: 'Enter', isComposing: true });
    expect(props.onRenameSession).not.toHaveBeenCalled();
    fireEvent.keyDown(title, { key: 'Enter' });
    expect(props.onRenameSession).toHaveBeenCalledWith('history', 'Renamed history');
    const cancelTitle = openRename();
    fireEvent.change(cancelTitle, { target: { value: 'Discarded' } });
    fireEvent.keyDown(cancelTitle, { key: 'Escape' });
    expect(props.onRenameSession).toHaveBeenCalledTimes(1);
    const actions = screen.getByRole('button', { name: 'Actions for History' });
    actions.focus();
    expect(document.activeElement).toBe(actions);
    fireEvent.click(actions);
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(screen.queryByRole('button', { name: 'Rename' })).toBeNull();
  });

  it('preserves navigation, export, pin, workspace and deletion callbacks', () => {
    const props = navigationProps();
    render(<Sidebar {...props} />);
    fireEvent.click(screen.getByRole('button', { name: 'History' }));
    expect(props.onSelectSession).toHaveBeenCalledWith('history');
    fireEvent.click(screen.getByTitle('New chat (Ctrl+N)'));
    expect(props.onNewSession).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', { name: 'Open global artifacts library' }));
    expect(props.onOpenArtifactsLibrary).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTitle('D:/project'));
    expect(props.onSelectWorkspace).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', { name: 'Detach workspace' }));
    expect(props.onClearWorkspace).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByRole('button', { name: 'Open settings' }));
    expect(props.onOpenSettings).toHaveBeenCalledOnce();
    for (const [name, format] of [['Export HTML (.html)', 'html'], ['Export Markdown', 'markdown'], ['Export JSON', 'json']]) {
      fireEvent.click(screen.getByRole('button', { name: 'Actions for History' }));
      fireEvent.click(screen.getByRole('button', { name }));
      expect(props.onExportSession).toHaveBeenCalledWith('history', format);
    }
    fireEvent.click(screen.getByRole('button', { name: 'Actions for History' }));
    fireEvent.click(screen.getByRole('button', { name: 'Pin chat to top' }));
    expect(props.onTogglePinSession).toHaveBeenCalledWith('history');
    fireEvent.click(screen.getByRole('button', { name: 'Actions for History' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(props.onDeleteSession).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(props.onDeleteSession).toHaveBeenCalledWith('history');
  });

  it('preserves Enter commit with IME protection and Git refresh', async () => {
    const props = navigationProps();
    const onCommitWorkspace = vi.fn().mockResolvedValue('abc');
    const onRefreshGitStatus = vi.fn().mockResolvedValue(null);
    render(<Sidebar {...props} gitStatus={{ isRepo: true, branch: 'main', hasChanges: true, summary: '1 change', files: [{ path: 'file.ts', status: 'M' }] }} onCommitWorkspace={onCommitWorkspace} onRefreshGitStatus={onRefreshGitStatus} />);
    fireEvent.click(screen.getByRole('button', { name: 'Toggle git details' }));
    fireEvent.click(screen.getByRole('button', { name: 'Refresh git status' }));
    expect(onRefreshGitStatus).toHaveBeenCalledOnce();
    const input = screen.getByPlaceholderText(/Commit message/i);
    fireEvent.change(input, { target: { value: '  fix: keyboard  ' } });
    fireEvent.keyDown(input, { key: 'Enter', isComposing: true });
    expect(onCommitWorkspace).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(screen.getByRole('button', { name: /Committing/ }).hasAttribute('disabled')).toBe(true);
    expect(input.closest('aside')?.innerHTML).not.toMatch(/animate-|shimmer|glow/);
    await waitFor(() => expect(onCommitWorkspace).toHaveBeenCalledWith('fix: keyboard'));
    await waitFor(() => expect(screen.queryByTestId('git-drawer')).toBeNull());
  });
});

it('opens the exact deep search hit rather than selecting the session tail', async () => {
  const hit = { sessionId: 'history', sessionTitle: 'History', sessionUpdatedAt: 2, snippet: 'needle', matchCount: 1, score: 1, matchMessageId: 'old-message' };
  const props = navigationProps();
  const jump = vi.fn(async () => true);
  render(<Sidebar {...props} onDeepSearch={async () => [hit]} onOpenSearchResult={jump} />);
  fireEvent.change(screen.getByPlaceholderText(/Search messages/i), { target: { value: 'needle' } });
  await waitFor(() => expect(screen.getByText('needle')).toBeTruthy());
  fireEvent.click(screen.getByText('needle'));
  expect(jump).toHaveBeenCalledWith(hit);
  expect(props.onSelectSession).not.toHaveBeenCalled();
});
