import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { InteractiveTerminal } from './InteractiveTerminal';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const mockedInvoke = vi.mocked(invoke);

async function runCommand(command: string) {
  const input = screen.getByRole('textbox', { name: 'Workspace shell command' }) as HTMLInputElement;
  fireEvent.change(input, { target: { value: command } });
  fireEvent.click(screen.getByRole('button', { name: 'Run' }));
  await screen.findByRole('button', { name: `Copy output for ${command}` });
}

describe('InteractiveTerminal accessibility and feedback', () => {
  beforeEach(() => mockedInvoke.mockReset());

  it('announces command history, exposes its time limit, and supports keyboard history', async () => {
    mockedInvoke.mockResolvedValue({ stdout: 'clean', stderr: '', exitCode: 0, durationMs: 4 });
    render(<InteractiveTerminal />);
    expect(screen.getByRole('log', { name: 'Shell command history' }).getAttribute('aria-live')).toBe('polite');
    expect(screen.getByText(/120 second limit/)).toBeTruthy();
    await runCommand('git status');
    const input = screen.getByRole('textbox', { name: 'Workspace shell command' }) as HTMLInputElement;
    expect(input.value).toBe('');
    fireEvent.keyDown(input, { key: 'ArrowUp' });
    expect(input.value).toBe('git status');
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(input.value).toBe('');
    fireEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(screen.queryByRole('button', { name: 'Copy output for git status' })).toBeNull();
  });

  it('shows a useful status while a bounded command runs', async () => {
    let finish!: (value: { stdout: string; stderr: string; exitCode: number; durationMs: number }) => void;
    mockedInvoke.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    render(<InteractiveTerminal />);
    fireEvent.change(screen.getByRole('textbox', { name: 'Workspace shell command' }), { target: { value: 'npm test' } });
    fireEvent.click(screen.getByRole('button', { name: 'Run' }));
    expect(screen.getByRole('status').textContent).toContain('stops after two minutes');
    const input = screen.getByRole('textbox', { name: 'Workspace shell command' }) as HTMLInputElement;
    expect(input.disabled).toBe(true);
    finish({ stdout: 'passed', stderr: '', exitCode: 0, durationMs: 10 });
    await screen.findByText('passed');
    expect(input.disabled).toBe(false);
  });

  it('makes copying keyboard accessible and reports clipboard failures', async () => {
    mockedInvoke.mockResolvedValue({ stdout: 'result text', stderr: '', exitCode: 0, durationMs: 4 });
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: vi.fn().mockRejectedValue(new Error('denied')) } });
    render(<InteractiveTerminal />);
    await runCommand('dir');
    const copy = screen.getByRole('button', { name: 'Copy output for dir' });
    expect(copy.className).toContain('focus-visible:opacity-100');
    fireEvent.focus(copy);
    fireEvent.click(copy);
    await screen.findByRole('alert');
    expect(screen.getByRole('alert').textContent).toContain('copy it manually');
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('keeps a failed command in the announced history', async () => {
    mockedInvoke.mockResolvedValue({ stdout: '', stderr: 'shell unavailable', exitCode: 1, durationMs: 2 });
    render(<InteractiveTerminal />);
    fireEvent.change(screen.getByRole('textbox', { name: 'Workspace shell command' }), { target: { value: 'pwd' } });
    fireEvent.click(screen.getByRole('button', { name: 'Run' }));
    await waitFor(() => expect(screen.getByRole('log').textContent).toContain('shell unavailable'));
    expect(screen.getByText('exit 1')).toBeTruthy();
  });
});
