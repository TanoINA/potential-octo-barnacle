import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
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
