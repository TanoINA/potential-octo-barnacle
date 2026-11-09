import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { FilesChangedCard } from './FilesChangedCard';
import type { ToolCallItem } from '../types/agent';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockImplementation((cmd: string) => {
    if (cmd === 'git_status') {
      return Promise.resolve({
        isRepo: true,
        branch: 'main',
        hasChanges: true,
        summary: '2 files changed',
      });
    }
    return Promise.resolve('ok');
  }),
}));

describe('FilesChangedCard minimization', () => {
  const sampleToolCalls: ToolCallItem[] = [
    {
      id: 'tool_1',
      name: 'write_file',
      args: { Path: 'src/app.py' },
      status: 'done',
      filesChanged: [
        {
          path: 'src/app.py',
          additions: 12,
          deletions: 2,
        },
        {
          path: 'src/utils.py',
          additions: 8,
          deletions: 0,
        },
      ],
    },
  ];

  it.each(['constructor', '__proto__', 'toString'])('aggregates repeated prototype-named path %s and reviews its latest artifact', (path) => {
    const selectArtifact = vi.fn();
    const { container } = render(
      <FilesChangedCard
        toolCalls={[
          { id: 'first', name: 'write_file', args: {}, status: 'done', artifactId: 'diff-first', filesChanged: [{ path, additions: 2, deletions: 1 }] },
          { id: 'second', name: 'edit_file', args: {}, status: 'done', artifactId: 'diff-latest', filesChanged: [{ path, additions: 3, deletions: 2 }] },
          { id: 'third', name: 'edit_file', args: {}, status: 'done', filesChanged: [{ path, additions: 1, deletions: 1 }] },
        ]}
        onSelectArtifact={selectArtifact}
      />,
    );
    expect(screen.getByText('1 file changed')).toBeTruthy();
    const rows = container.querySelectorAll<HTMLElement>('[data-file-link="true"]');
    expect(rows).toHaveLength(1);
    expect(rows[0].getAttribute('data-file-path')).toBe(path);
    expect(within(rows[0]).getByText(path)).toBeTruthy();
    expect(within(rows[0]).getByText('+6')).toBeTruthy();
    expect(within(rows[0]).getByText('-4')).toBeTruthy();
    fireEvent.click(within(rows[0]).getByRole('button', { name: 'Review' }));
    expect(selectArtifact).toHaveBeenCalledExactlyOnceWith('diff-latest');
  });

  it('keeps unscoped historical receipts review-only even in an active Git workspace', async () => {
    vi.mocked(invoke).mockClear();
    const selectArtifact = vi.fn();
    render(<FilesChangedCard toolCalls={sampleToolCalls.map((tool) => ({ ...tool, artifactId: 'diff-original' }))} onSelectArtifact={selectArtifact} />);
    await act(async () => {});
    expect(invoke).not.toHaveBeenCalled();
    expect(screen.getByText(/review-only.*workspace/i)).toBeTruthy();
    expect(screen.queryByText('main')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Commit' })).toBeNull();
    const accepts = screen.getAllByRole('button', { name: 'Accept' }) as HTMLButtonElement[];
    const reverts = screen.getAllByRole('button', { name: 'Revert' }) as HTMLButtonElement[];
    expect([...accepts, ...reverts].every((button) => button.disabled)).toBe(true);
    [...accepts, ...reverts].forEach((button) => fireEvent.click(button));
    expect(invoke).not.toHaveBeenCalled();
    fireEvent.click(screen.getAllByRole('button', { name: 'Review' })[0]);
    expect(selectArtifact).toHaveBeenCalledWith('diff-original');
  });

  it('does not query the workspace for an empty receipt or on receipt updates', async () => {
    vi.mocked(invoke).mockClear();
    const { container, rerender } = render(<FilesChangedCard toolCalls={[]} />);
    await act(async () => {});
    expect(container.childElementCount).toBe(0);
    expect(invoke).not.toHaveBeenCalled();
    rerender(<FilesChangedCard toolCalls={sampleToolCalls} />);
    await act(async () => {});
    expect(screen.getByText('2 files changed')).toBeTruthy();
    expect(invoke).not.toHaveBeenCalled();
  });

  it('renders files changed summary and can be minimized and expanded', () => {
    render(<FilesChangedCard toolCalls={sampleToolCalls} />);

    // Initial state: expanded
    expect(screen.getByText('2 files changed')).toBeTruthy();
    expect(screen.getByText('+20')).toBeTruthy();
    expect(screen.getAllByText('-2').length).toBeGreaterThan(0);
    expect(screen.getByText('app.py')).toBeTruthy();
    expect(screen.getByText('utils.py')).toBeTruthy();

    // Minimize via header minimize button
    const minimizeBtn = screen.getByTitle('Minimize');
    fireEvent.click(minimizeBtn);

    // After minimize: file rows are hidden, minimized badge is shown
    expect(screen.queryByText('app.py')).toBeNull();
    expect(screen.queryByText('utils.py')).toBeNull();
    expect(screen.getByText('(minimized)')).toBeTruthy();

    // Expand via expand button
    const expandBtn = screen.getByTitle('Expand');
    fireEvent.click(expandBtn);

    // Expanded again: file rows are visible
    expect(screen.getByText('app.py')).toBeTruthy();
    expect(screen.getByText('utils.py')).toBeTruthy();
    expect(screen.queryByText('(minimized)')).toBeNull();
  });

  it('toggles minimize state when clicking the header row', () => {
    render(<FilesChangedCard toolCalls={sampleToolCalls} />);

    expect(screen.getByText('app.py')).toBeTruthy();

    // Click header title area to minimize
    fireEvent.click(screen.getByText('2 files changed'));
    expect(screen.queryByText('app.py')).toBeNull();

    // Click again to expand
    fireEvent.click(screen.getByText('2 files changed'));
    expect(screen.getByText('app.py')).toBeTruthy();
  });
});
