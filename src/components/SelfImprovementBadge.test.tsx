import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { SelfImprovementEvent } from '../types/agent';
import { SelfImprovementBadge } from './SelfImprovementBadge';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const created: SelfImprovementEvent = {
  run_id: 'run-1',
  session_id: 'session-1',
  action: 'skill_created',
  skill_name: 'test-recipe',
  summary: 'A reusable test recipe',
};

beforeEach(() => { vi.mocked(invoke).mockReset(); });

describe('SelfImprovementBadge', () => {
  it('keeps a fresh inspector unchanged when a dismissed request finishes late', async () => {
    let rejectDismissed!: (reason: string) => void;
    let resolveFresh!: (content: string) => void;
    vi.mocked(invoke)
      .mockImplementationOnce(() => new Promise<string>((_resolve, reject) => { rejectDismissed = reject; }))
      .mockImplementationOnce(() => new Promise<string>((resolve) => { resolveFresh = resolve; }));
    render(<SelfImprovementBadge review={created} />);
    const trigger = screen.getByRole('button', { name: "Self-improvement review · Skill 'test-recipe' created" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole('button', { name: 'Close skill inspector' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    fireEvent.click(trigger);
    await act(async () => { rejectDismissed('Dismissed request error'); });
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.getByRole('status')).toBeTruthy();
    await act(async () => { resolveFresh('# Fresh read\n'); });
    expect(screen.getByRole('dialog').querySelector('pre code')?.textContent).toBe('# Fresh read\n');
  });

  it.each(['resolve', 'reject'] as const)('safely ignores a pending %s after the badge unmounts', async (outcome) => {
    let resolveRead!: (content: string) => void;
    let rejectRead!: (reason: string) => void;
    vi.mocked(invoke).mockImplementationOnce(() => new Promise<string>((resolve, reject) => {
      resolveRead = resolve;
      rejectRead = reject;
    }));
    const { unmount } = render(<SelfImprovementBadge review={created} />);
    const trigger = screen.getByRole('button', { name: "Self-improvement review · Skill 'test-recipe' created" });
    trigger.focus();
    fireEvent.click(trigger);
    unmount();
    await act(async () => {
      if (outcome === 'resolve') resolveRead('# Obsolete read\n');
      else rejectRead('Obsolete error');
    });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(screen.queryByRole('alert')).toBeNull();
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it('allows dismissal of native string errors with Escape and restores the trigger focus', async () => {
    vi.mocked(invoke).mockRejectedValueOnce('Skill no longer exists');
    render(<SelfImprovementBadge review={created} />);
    const trigger = screen.getByRole('button', { name: "Self-improvement review · Skill 'test-recipe' created" });
    trigger.focus();
    fireEvent.click(trigger);
    expect((await screen.findByRole('alert')).textContent).toContain('Skill no longer exists');
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Close skill inspector' })));
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it('constrains long names to wrapping inline text without a pill or block background', () => {
    const skill_name = 'a-long-reusable-skill-name-with-many-repeatable-steps';
    render(<SelfImprovementBadge review={{ ...created, skill_name }} />);
    const trigger = screen.getByRole('button', { name: `Self-improvement review · Skill '${skill_name}' created` });
    expect(trigger.className).toContain('max-w-full');
    expect(trigger.className).toContain('min-w-0');
    expect(trigger.className).toContain('bg-transparent');
    expect(trigger.className).not.toContain('rounded');
    expect(trigger.querySelector('span')?.className).toContain('[overflow-wrap:anywhere]');
    expect(trigger.querySelector('svg')?.classList.contains('lucide-brain')).toBe(true);
  });

  it('refreshes an open inspector for a new review run even when the skill name is unchanged', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('# First revision\n').mockResolvedValueOnce('# Latest revision\n');
    const { rerender } = render(<SelfImprovementBadge review={created} />);
    fireEvent.click(screen.getByRole('button', { name: "Self-improvement review · Skill 'test-recipe' created" }));
    await waitFor(() => expect(screen.getByRole('dialog').querySelector('pre code')?.textContent).toBe('# First revision\n'));

    rerender(<SelfImprovementBadge review={{ ...created, run_id: 'run-2', action: 'skill_updated' }} />);
    await waitFor(() => expect(screen.getByRole('dialog').querySelector('pre code')?.textContent).toBe('# Latest revision\n'));
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('does not reopen an inspector without a click after switching through NO_OP', () => {
    vi.mocked(invoke).mockImplementation(() => new Promise<string>(() => {}));
    const { rerender } = render(<SelfImprovementBadge review={created} />);
    fireEvent.click(screen.getByRole('button', { name: "Self-improvement review · Skill 'test-recipe' created" }));
    expect(screen.getByRole('dialog')).toBeTruthy();
    rerender(<SelfImprovementBadge review={{ ...created, action: 'no_action' }} />);
    expect(screen.queryByRole('dialog')).toBeNull();
    rerender(<SelfImprovementBadge review={created} />);
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it('ignores stale reads after the inspector is retargeted to another session and skill', async () => {
    let rejectOld!: (reason: Error) => void;
    let resolveCurrent!: (content: string) => void;
    vi.mocked(invoke)
      .mockImplementationOnce(() => new Promise<string>((_resolve, reject) => { rejectOld = reject; }))
      .mockImplementationOnce(() => new Promise<string>((resolve) => { resolveCurrent = resolve; }));
    const { rerender } = render(<SelfImprovementBadge review={created} />);
    fireEvent.click(screen.getByRole('button', { name: "Self-improvement review · Skill 'test-recipe' created" }));
    rerender(<SelfImprovementBadge review={{ ...created, run_id: 'run-2', session_id: 'session-2', skill_name: 'new-recipe' }} />);
    const dialog = screen.getByRole('dialog', { name: 'Self-improvement skill: new-recipe' });

    await act(async () => { resolveCurrent('# Current recipe\n'); });
    expect(dialog.querySelector('pre code')?.textContent).toBe('# Current recipe\n');
    await act(async () => { rejectOld(new Error('Stale failure')); });
    expect(dialog.querySelector('pre code')?.textContent).toBe('# Current recipe\n');
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryByRole('status')).toBeNull();
    expect(invoke).toHaveBeenLastCalledWith('read_reflection_skill', { runId: 'run-2', sessionId: 'session-2', skillName: 'new-recipe' });
  });

  it('shows read errors with a retry that reuses only the scoped skill identifier', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('Skill is temporarily unavailable'));
    render(<SelfImprovementBadge review={{ ...created, action: 'skill_updated' }} />);
    fireEvent.click(screen.getByRole('button', { name: "Self-improvement review · Skill 'test-recipe' updated" }));

    expect((await screen.findByRole('alert')).textContent).toContain('Skill is temporarily unavailable');
    expect(screen.queryByRole('status')).toBeNull();
    vi.mocked(invoke).mockResolvedValueOnce('# Recovered skill\n');
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));

    await waitFor(() => expect(screen.getByRole('dialog').querySelector('pre code')?.textContent).toBe('# Recovered skill\n'));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ['read_reflection_skill', { runId: 'run-1', sessionId: 'session-1', skillName: 'test-recipe' }],
      ['read_reflection_skill', { runId: 'run-1', sessionId: 'session-1', skillName: 'test-recipe' }],
    ]);
    fireEvent.click(screen.getByRole('button', { name: 'Close skill inspector' }));
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('opens a scoped inspector and displays the full SKILL.md as inert raw text', async () => {
    const rawSkill = '---\nname: test-recipe\n---\n# Recipe\n\n    keep  spacing\n<script>window.injected = true</script>\n<img src=x onerror="alert(1)">\n[unsafe](javascript:alert(1))\n';
    let resolveRead!: (content: string) => void;
    vi.mocked(invoke).mockImplementation(() => new Promise<string>((resolve) => { resolveRead = resolve; }));
    const { container } = render(<SelfImprovementBadge review={created} />);
    const trigger = screen.getByRole('button', { name: "Self-improvement review · Skill 'test-recipe' created" });
    trigger.focus();
    fireEvent.click(trigger);

    const dialog = screen.getByRole('dialog', { name: 'Self-improvement skill: test-recipe' });
    expect(container.contains(dialog)).toBe(false);
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(screen.getByRole('status').textContent).toContain('Loading SKILL.md');
    expect(screen.getByText('.aether/skills/test-recipe/SKILL.md')).toBeTruthy();
    expect(invoke).toHaveBeenCalledExactlyOnceWith('read_reflection_skill', {
      runId: 'run-1', sessionId: 'session-1', skillName: 'test-recipe',
    });
    expect(dialog.querySelector('input')).toBeNull();

    resolveRead(rawSkill);
    await waitFor(() => expect(dialog.querySelector('pre code')?.textContent).toBe(rawSkill));
    expect(dialog.querySelector('script, img, a')).toBeNull();
    expect(screen.queryByRole('status')).toBeNull();
    await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true));
    fireEvent.keyDown(document, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it('renders an updated skill with the updated label', () => {
    render(<SelfImprovementBadge review={{ ...created, action: 'skill_updated' }} />);
    expect(screen.getByRole('button', { name: "Self-improvement review · Skill 'test-recipe' updated" })).toBeTruthy();
  });

  it('renders NO_OP as a non-interactive review even if a name is present', () => {
    render(<SelfImprovementBadge review={{ ...created, action: 'no_action' }} />);
    expect(screen.getByText('Self-improvement review · No reusable lesson found')).toBeTruthy();
    expect(screen.queryByRole('button')).toBeNull();
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it.each([undefined, '', '   '])('hides invalid success without a usable name: %s', (skill_name) => {
    const { container } = render(<SelfImprovementBadge review={{ ...created, skill_name }} />);
    expect(container.textContent).toBe('');
    expect(screen.queryByRole('button')).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it('offers a created skill as a compact inline review without reading it before a click', () => {
    render(<SelfImprovementBadge review={created} />);

    const trigger = screen.getByRole('button', {
      name: "Self-improvement review · Skill 'test-recipe' created",
    });
    expect(trigger.getAttribute('aria-haspopup')).toBe('dialog');
    expect(trigger.textContent).toBe("Self-improvement review · Skill 'test-recipe' created");
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});
