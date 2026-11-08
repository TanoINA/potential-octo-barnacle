import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { SubagentTreeCard } from './SubagentTreeCard';
import type { SubagentProgress } from '../types/agent';

describe('SubagentTreeCard', () => {
  const sampleSubagents: SubagentProgress[] = [
    {
      id: 'sub-1',
      role: 'Researcher',
      task: 'Investigate architecture specifications',
      status: 'completed',
      startedAt: Date.now() - 5000,
      completedAt: Date.now(),
      summary: 'Found 3 key design patterns',
      artifactId: 'art-1',
    },
    {
      id: 'sub-2',
      role: 'Code Generator',
      task: 'Generate Rust traits',
      status: 'running',
      startedAt: Date.now() - 2000,
    },
  ];

  it('shows failed and error statuses in a red header count without counting cancellations', () => {
    const statuses = ['completed', 'running', 'failed', 'error', 'cancelled'] as const;
    const subagents: SubagentProgress[] = statuses.map((status, index) => ({
      ...sampleSubagents[0], id: `sub-${index}`, status,
    }));
    render(<SubagentTreeCard subagents={subagents} />);
    const failures = screen.getByText('· 2 failed');
    expect(failures.classList.contains('text-[#f85149]')).toBe(true);
    expect(failures.parentElement?.textContent).toContain('1/5 done');
    expect(failures.parentElement?.textContent).toContain('1 cancelled');
    expect(screen.getByText('1 active')).toBeTruthy();
    expect(screen.queryByText('Researcher')).toBeNull();
  });

  it('does not show a failure indicator when no subagents failed', () => {
    render(<SubagentTreeCard subagents={sampleSubagents} />);
    expect(screen.queryByText(/failed/)).toBeNull();
  });

  it('renders nothing when subagent list is empty', () => {
    const { container } = render(<SubagentTreeCard subagents={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders subagent tree card with task counts and collapsed by default', () => {
    render(<SubagentTreeCard subagents={sampleSubagents} />);
    expect(screen.getByText('Subagent Tasks')).toBeTruthy();
    expect(screen.getByText('1/2 done')).toBeTruthy();
    expect(screen.getByText('1 active')).toBeTruthy();
    // Collapsed by default
    expect(screen.queryByText('Researcher')).toBeNull();

    // Expand to see details
    fireEvent.click(screen.getByText('Subagent Tasks'));
    expect(screen.getByText('Researcher')).toBeTruthy();
    expect(screen.getByText('Code Generator')).toBeTruthy();
    expect(screen.getByText('Found 3 key design patterns')).toBeTruthy();
  });

  it('toggles expansion when header is clicked', () => {
    render(<SubagentTreeCard subagents={sampleSubagents} />);
    const header = screen.getByText('Subagent Tasks');
    expect(screen.queryByText('Investigate architecture specifications')).toBeNull();

    fireEvent.click(header);
    expect(screen.getByText('Investigate architecture specifications')).toBeTruthy();

    fireEvent.click(header);
    expect(screen.queryByText('Investigate architecture specifications')).toBeNull();
  });

  it('calls onSelectArtifact when view artifact button is clicked', () => {
    const select = vi.fn();
    render(<SubagentTreeCard subagents={sampleSubagents} onSelectArtifact={select} />);
    // Expand first
    fireEvent.click(screen.getByText('Subagent Tasks'));
    const button = screen.getByText('View Generated Artifact');
    fireEvent.click(button);
    expect(select).toHaveBeenCalledWith('art-1');
  });

  it('renders cancelled status for cancelled subagent', () => {
    const cancelledSubagents: SubagentProgress[] = [
      {
        id: 'sub-1',
        role: 'Auditor',
        task: 'Audit codebase',
        status: 'cancelled',
        startedAt: Date.now() - 3000,
        completedAt: Date.now(),
      },
    ];
    render(<SubagentTreeCard subagents={cancelledSubagents} />);
    expect(screen.getByText(/0\/1 done · 1 cancelled/)).toBeTruthy();
    // Expand to verify body item
    fireEvent.click(screen.getByText('Subagent Tasks'));
    expect(screen.getByText('cancelled')).toBeTruthy();
  });
});
