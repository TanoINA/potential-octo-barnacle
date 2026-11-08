import { fireEvent, render, screen, waitFor } from '@testing-library/react';
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

  it('keeps unscoped historical receipts review-only even in an active Git workspace', async () => {
    vi.mocked(invoke).mockClear();
    const selectArtifact = vi.fn();
    render(<FilesChangedCard toolCalls={sampleToolCalls.map((tool) => ({ ...tool, artifactId: 'diff-original' }))} onSelectArtifact={selectArtifact} />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('git_status'));
    expect(screen.getByText(/review-only.*workspace/i)).toBeTruthy();
    const accepts = screen.getAllByRole('button', { name: 'Accept' }) as HTMLButtonElement[];
    const reverts = screen.getAllByRole('button', { name: 'Revert' }) as HTMLButtonElement[];
    const commit = screen.getByRole('button', { name: 'Commit' }) as HTMLButtonElement;
    expect([...accepts, ...reverts, commit].every((button) => button.disabled)).toBe(true);
    [...accepts, ...reverts, commit].forEach((button) => fireEvent.click(button));
    expect(vi.mocked(invoke).mock.calls.every(([command]) => command === 'git_status')).toBe(true);
    fireEvent.click(screen.getAllByRole('button', { name: 'Review' })[0]);
    expect(selectArtifact).toHaveBeenCalledWith('diff-original');
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
