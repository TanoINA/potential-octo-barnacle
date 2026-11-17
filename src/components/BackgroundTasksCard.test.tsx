import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { BackgroundTasksCard } from './BackgroundTasksCard';
import type { BackgroundTask } from '../types/agent';

describe('BackgroundTasksCard', () => {
  const sampleTasks: BackgroundTask[] = [
    {
      sessionId: 'test-session',
      id: 'task_1',
      command: 'npm run dev',
      toolSummary: 'Dev server startup',
      isDaemon: true,
      status: 'running',
      pid: 1234,
      startedAt: Date.now() - 5000,
      logPath: '/tmp/tasks/task_1.log',
      recentOutput: 'Vite v5 ready on http://localhost:5173',
    },
    {
      sessionId: 'test-session',
      id: 'task_2',
      command: 'npm run build',
      toolSummary: 'Background build',
      isDaemon: false,
      status: 'completed',
      pid: 5678,
      startedAt: Date.now() - 10000,
      completedAt: Date.now() - 1000,
      exitCode: 0,
      logPath: '/tmp/tasks/task_2.log',
      recentOutput: 'Build finished in 4.2s',
    },
  ];

  it('keeps stopping tasks active and disables their stop control', () => {
    const kill = vi.fn();
    render(<BackgroundTasksCard tasks={[{ ...sampleTasks[0], status: 'stopping' }]} onKillTask={kill} />);
    expect(screen.getByText('1 active')).toBeTruthy();
    fireEvent.click(screen.getByText('Background Tasks'));
    const stopping = screen.getByRole('button', { name: /Stopping/ });
    expect((stopping as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(stopping);
    expect(kill).not.toHaveBeenCalled();
  });

  it('copies the exact command without opening logs or killing the task, and waits for success', async () => {
    let resolve!: () => void;
    const writeText = vi.fn(() => new Promise<void>((done) => { resolve = done; }));
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
    const kill = vi.fn();
    render(<BackgroundTasksCard tasks={[sampleTasks[0]]} onKillTask={kill} />);
    fireEvent.click(screen.getByText('Background Tasks'));
    fireEvent.click(screen.getByRole('button', { name: 'Copy command' }));
    expect(writeText).toHaveBeenCalledExactlyOnceWith('npm run dev');
    expect(screen.queryByText('Copied')).toBeNull();
    expect(screen.queryByText(sampleTasks[0].recentOutput)).toBeNull();
    expect(kill).not.toHaveBeenCalled();
    await act(async () => resolve());
    expect(screen.getByText('Copied')).toBeTruthy();
    fireEvent.click(screen.getByText('View Logs'));
    expect(screen.getByRole('button', { name: 'Copy logs' }).textContent).not.toContain('Copied');
  });

  it.each(['rejected', 'unavailable'])('reports %s clipboard errors for commands and logs', async (mode) => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: mode === 'unavailable' ? undefined : { writeText: vi.fn().mockRejectedValue(new Error('denied')) },
    });
    render(<BackgroundTasksCard tasks={[sampleTasks[0]]} />);
    fireEvent.click(screen.getByText('Background Tasks'));
    fireEvent.click(screen.getByRole('button', { name: 'Copy command' }));
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('Could not copy'));
    expect(screen.queryByText('Copied')).toBeNull();
    fireEvent.click(screen.getByText('View Logs'));
    fireEvent.click(screen.getByRole('button', { name: 'Copy logs' }));
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('Could not copy'));
    expect(screen.queryByText('Copied')).toBeNull();
  });

  it('renders nothing when task list is empty', () => {
    const { container } = render(<BackgroundTasksCard tasks={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders card header with summary and toggles expansion', () => {
    const killFn = vi.fn();
    render(<BackgroundTasksCard tasks={sampleTasks} onKillTask={killFn} />);
    expect(screen.getByText('Background Tasks')).toBeTruthy();
    expect(screen.getByText('1/2 done')).toBeTruthy();
    expect(screen.getByText('1 active')).toBeTruthy();

    // Collapsed by default
    expect(screen.queryByText('Dev server startup')).toBeNull();

    // Expand
    fireEvent.click(screen.getByText('Background Tasks'));
    expect(screen.getByText('Dev server startup')).toBeTruthy();
    expect(screen.getByText('PID 1234')).toBeTruthy();
    expect(screen.getByText('Background build')).toBeTruthy();
    expect(screen.getByText('Kill')).toBeTruthy();

    // Click Kill
    fireEvent.click(screen.getByText('Kill'));
    expect(killFn).toHaveBeenCalledWith('task_1');

    // Toggle logs
    const viewLogsButtons = screen.getAllByText('View Logs');
    fireEvent.click(viewLogsButtons[0]);
    expect(screen.getByText('Vite v5 ready on http://localhost:5173')).toBeTruthy();
  });
});
