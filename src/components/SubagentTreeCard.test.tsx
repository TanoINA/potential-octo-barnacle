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

  it('renders nothing when subagent list is empty', () => {
    const { container } = render(<SubagentTreeCard subagents={[]} />);
    expect(container.firstChild).toBeNull();
  });

  it('renders subagent tree card with task counts and details', () => {
    render(<SubagentTreeCard subagents={sampleSubagents} />);
    expect(screen.getByText('Subagent Tasks')).toBeTruthy();
    expect(screen.getByText('1/2 done')).toBeTruthy();
    expect(screen.getByText('1 active')).toBeTruthy();
    expect(screen.getByText('Researcher')).toBeTruthy();
    expect(screen.getByText('Code Generator')).toBeTruthy();
    expect(screen.getByText('Found 3 key design patterns')).toBeTruthy();
  });

  it('toggles expansion when header is clicked', () => {
    render(<SubagentTreeCard subagents={sampleSubagents} />);
    const header = screen.getByText('Subagent Tasks');
    expect(screen.getByText('Investigate architecture specifications')).toBeTruthy();

    fireEvent.click(header);
    expect(screen.queryByText('Investigate architecture specifications')).toBeNull();

    fireEvent.click(header);
    expect(screen.getByText('Investigate architecture specifications')).toBeTruthy();
  });

  it('calls onSelectArtifact when view artifact button is clicked', () => {
    const select = vi.fn();
    render(<SubagentTreeCard subagents={sampleSubagents} onSelectArtifact={select} />);
    const button = screen.getByText('View Generated Artifact');
    fireEvent.click(button);
    expect(select).toHaveBeenCalledWith('art-1');
  });
});
