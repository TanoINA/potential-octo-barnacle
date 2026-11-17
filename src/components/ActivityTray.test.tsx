import { fireEvent, render, screen, within } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import type { BackgroundTask, SkillInfo, SubagentProgress } from '../types/agent';
import { ChatCanvas } from './ChatCanvas';

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
});

const skills: SkillInfo[] = Array.from({ length: 100 }, (_, index) => ({
  name: `skill-${index}`, description: `Skill description ${index}`,
  path: `D:/skills/skill-${index}/SKILL.md`, source: 'workspace',
}));
const tasks: BackgroundTask[] = Array.from({ length: 100 }, (_, index) => ({
  sessionId: 'session-a', id: `task-${index}`, command: `command-${index}`,
  toolSummary: `Background job ${index}`, isDaemon: true,
  status: index === 0 ? 'running' : index === 1 ? 'failed' : 'completed',
  pid: 1000 + index, startedAt: 1700000000000, logPath: `D:/logs/task-${index}.log`,
  recentOutput: `Output from task ${index}`,
}));
const subagents: SubagentProgress[] = Array.from({ length: 100 }, (_, index) => ({
  id: `sub-${index}`, role: `Worker ${index}`, task: `Investigate module ${index}`,
  status: index === 0 ? 'running' : index === 1 ? 'error' : 'completed',
  startedAt: 1700000000000, summary: `Findings from worker ${index}`, artifactId: `artifact-${index}`,
}));

function canvasProps(overrides: Partial<ComponentProps<typeof ChatCanvas>> = {}): ComponentProps<typeof ChatCanvas> {
  return {
    messages: [], isStreaming: true, stopRequested: false,
    onSendMessage: vi.fn(async () => true), onStop: vi.fn(), onSelectArtifact: vi.fn(),
    onClearSession: vi.fn(), onNewSession: () => 'new', onOpenSettings: vi.fn(),
    onOpenModelPicker: vi.fn(), effectiveConfig: null, sessionTitle: 'Activity test',
    artifactCount: 100, isArtifactOpen: false, onToggleArtifacts: vi.fn(), onListFiles: async () => [],
    currentSessionId: 'session-a', subagents, backgroundTasks: tasks, activeSkills: skills,
    onKillBackgroundTask: vi.fn(), onRefreshBackgroundTasks: vi.fn(),
    runStatus: {
      runId: 'run-a', sessionId: 'session-a', origin: 'chat', phase: 'running_tool',
      startedAt: Date.now(), phaseSince: Date.now(),
    },
    ...overrides,
  };
}

describe('bounded chat activity', () => {
  it('retains real task refresh, kill, and logs with a small fixture', () => {
    const props = canvasProps({ backgroundTasks: [tasks[0], tasks[1], tasks[99]], subagents: [], activeSkills: [] });
    render(<ChatCanvas {...props} />);
    fireEvent.click(screen.getByRole('button', { name: /Activity.*3 tasks/i }));
    const panel = screen.getByRole('region', { name: 'Activity details' });
    fireEvent.click(within(panel).getByRole('button', { name: 'Refresh tasks' }));
    expect(props.onRefreshBackgroundTasks).toHaveBeenCalledOnce();
    fireEvent.click(within(panel).getByRole('button', { name: 'Expand tasks' }));
    const taskCard = within(panel).getByTestId('background-tasks-card');
    expect(within(taskCard).getAllByText(/^Background job /)).toHaveLength(3);
    expect(within(taskCard).getByText('PID 1099')).toBeTruthy();
    expect(within(taskCard).getByText('command-99')).toBeTruthy();
    expect(within(taskCard).getByText('D:/logs/task-99.log')).toBeTruthy();
    expect(within(taskCard).getAllByRole('button', { name: 'Kill' })).toHaveLength(1);
    fireEvent.click(within(taskCard).getByRole('button', { name: 'Kill' }));
    expect(props.onKillBackgroundTask).toHaveBeenCalledExactlyOnceWith('task-0');
    fireEvent.click(within(taskCard).getAllByRole('button', { name: 'View Logs' })[2]);
    expect(within(taskCard).getByText('Output from task 99')).toBeTruthy();
    expect(within(taskCard).getByTitle('Copy logs')).toBeTruthy();
  });

  it('retains subagent artifact navigation with a small fixture', () => {
    const props = canvasProps({ subagents: [subagents[0], subagents[1], subagents[99]], backgroundTasks: [], activeSkills: [] });
    render(<ChatCanvas {...props} />);
    fireEvent.click(screen.getByRole('button', { name: /Activity.*3 subagents/i }));
    const panel = screen.getByRole('region', { name: 'Activity details' });
    fireEvent.click(within(panel).getByRole('button', { name: 'Expand subagents' }));
    const subagentCard = within(panel).getByTestId('subagent-tree-card');
    expect(within(subagentCard).getAllByText(/^Worker /)).toHaveLength(3);
    expect(within(subagentCard).getByText('Investigate module 99')).toBeTruthy();
    expect(within(subagentCard).getByText('Findings from worker 99')).toBeTruthy();
    fireEvent.click(within(subagentCard).getAllByRole('button', { name: 'View Generated Artifact' })[2]);
    expect(props.onSelectArtifact).toHaveBeenCalledExactlyOnceWith('artifact-99');
  });

  it('keeps 100 tasks of each kind bounded and closes on session navigation without reopening on updates or return', () => {
    const props = canvasProps();
    const { rerender } = render(<ChatCanvas {...props} />);
    fireEvent.click(screen.getByRole('button', { name: /Activity.*100 subagents/i }));
    const panel = screen.getByRole('region', { name: 'Activity details' });
    expect(panel).toBeTruthy();
    fireEvent.click(within(panel).getByRole('button', { name: 'Expand tasks' }));
    const taskCard = within(panel).getByTestId('background-tasks-card');
    expect(within(taskCard).getAllByText(/^Background job /)).toHaveLength(100);
    fireEvent.click(within(panel).getByRole('button', { name: 'Expand subagents' }));
    const subagentCard = within(panel).getByTestId('subagent-tree-card');
    expect(within(subagentCard).getAllByText(/^Worker /)).toHaveLength(100);
    expect(screen.getByRole('button', { name: /Activity.*100 subagents/i }).getAttribute('aria-expanded')).toBe('true');
    expect(panel.classList.contains('max-h-[min(320px,35vh)]')).toBe(true);
    rerender(<ChatCanvas {...props} currentSessionId="session-b" />);
    expect(screen.queryByRole('region', { name: 'Activity details' })).toBeNull();
    const updated = {
      ...props, currentSessionId: 'session-b',
      backgroundTasks: tasks.map((task) => ({ ...task, sessionId: 'session-b', recentOutput: `${task.recentOutput}\nUpdated` })),
      activeSkills: [...skills, { ...skills[0], name: 'new-skill', path: 'D:/skills/new/SKILL.md' }],
      subagents: subagents.map((subagent) => ({ ...subagent, summary: `${subagent.summary}\nUpdated` })),
    };
    rerender(<ChatCanvas {...updated} />);
    const toggle = screen.getByRole('button', { name: /Activity.*101 skills/i });
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(toggle);
    rerender(<ChatCanvas {...updated} backgroundTasks={[...updated.backgroundTasks]} />);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    fireEvent.click(toggle);
    rerender(<ChatCanvas {...updated} activeSkills={[...updated.activeSkills]} />);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    rerender(<ChatCanvas {...props} />);
    expect(screen.getByRole('button', { name: /Activity.*100 skills/i }).getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('region', { name: 'Activity details' })).toBeNull();
  });

  it('contains all three detail sections in one height-bounded scroll panel without hiding run controls', () => {
    const props = canvasProps();
    render(<ChatCanvas {...props} />);
    const toggle = screen.getByRole('button', { name: /Activity.*100 subagents/i });
    toggle.focus();
    expect(document.activeElement).toBe(toggle);
    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    const panel = screen.getByRole('region', { name: 'Activity details' });
    expect(toggle.getAttribute('aria-controls')).toBe(panel.id);
    expect(panel.classList.contains('max-h-[min(320px,35vh)]')).toBe(true);
    expect(panel.classList.contains('overflow-y-auto')).toBe(true);
    expect(panel.classList.contains('overflow-x-hidden')).toBe(true);
    expect(panel.classList.contains('overscroll-contain')).toBe(true);
    expect(panel.tabIndex).toBe(0);
    expect(panel.parentElement?.className).not.toMatch(/absolute|fixed/);
    expect(within(panel).getByText('Subagent Tasks')).toBeTruthy();
    expect(within(panel).getByText('Background Tasks')).toBeTruthy();
    expect(within(panel).getByRole('heading', { name: 'Active Skills (100)' })).toBeTruthy();
    const skillList = within(panel).getByRole('list', { name: 'Active skills' });
    expect(within(skillList).getAllByRole('listitem')).toHaveLength(100);
    expect(within(skillList).getByText('skill-99')).toBeTruthy();
    expect(within(skillList).getByText('Skill description 99')).toBeTruthy();
    expect(within(skillList).getByText('D:/skills/skill-99/SKILL.md')).toBeTruthy();
    expect(within(skillList).getAllByText('workspace')).toHaveLength(100);
    expect(panel.contains(screen.getByText('Running tool'))).toBe(false);
    const composer = screen.getByRole('textbox', { name: 'Message AetherAgent' }) as HTMLTextAreaElement;
    expect(panel.contains(composer)).toBe(false);
    composer.focus();
    fireEvent.change(composer, { target: { value: 'Still editable with details open' } });
    expect(document.activeElement).toBe(composer);
    expect(composer.value).toBe('Still editable with details open');
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
    expect(props.onStop).toHaveBeenCalledOnce();
    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('region', { name: 'Activity details' })).toBeNull();
  });

  it('collapses 100 skills and 100 tasks of each kind into one non-wrapping summary row', () => {
    render(<ChatCanvas {...canvasProps()} />);
    expect(screen.queryAllByText(/^Active Skill:/)).toHaveLength(0);
    const toggle = screen.getByRole('button', { name: /Activity.*100 subagents.*100 tasks.*100 skills/i });
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(toggle.classList.contains('h-8')).toBe(true);
    expect(toggle.classList.contains('flex-nowrap')).toBe(true);
    expect(toggle.classList.contains('whitespace-nowrap')).toBe(true);
    expect(toggle.classList.contains('overflow-hidden')).toBe(true);
    expect(toggle.textContent).toContain('2 running');
    expect(toggle.textContent).toContain('2 failed');
    expect(screen.queryByTestId('subagent-tree-card')).toBeNull();
    expect(screen.queryByTestId('background-tasks-card')).toBeNull();
    expect(screen.queryByRole('region', { name: 'Activity details' })).toBeNull();
    expect(screen.getByText('Running tool')).toBeTruthy();
    const composer = screen.getByRole('textbox', { name: 'Message AetherAgent' }) as HTMLTextAreaElement;
    fireEvent.change(composer, { target: { value: 'Keep this draft usable' } });
    expect(composer.disabled).toBe(false);
    expect(composer.value).toBe('Keep this draft usable');
    expect(within(toggle).queryAllByRole('button')).toHaveLength(0);
  });
});
