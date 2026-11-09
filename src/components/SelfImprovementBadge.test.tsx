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
  it('keeps a local approval across an equivalent stale event rerender but does not infer approval after remount', async () => {
    let approved = false;
    const proposal = { id: 'pending-1', skill_name: 'test-recipe', file_path: '/skill/SKILL.md', before: '', after: 'new', diff: '+new' };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'list_review_proposals') return approved ? [] : [proposal];
      if (command === 'review_proposal_diff') return proposal;
      if (command === 'approve_review_proposal') { approved = true; return { action: 'skill_updated', skill_name: 'test-recipe', file_path: '/skill/SKILL.md' }; }
      if (command === 'read_reflection_skill') return '# Approved';
      throw new Error('Unexpected command');
    });
    const stale: SelfImprovementEvent = { ...created, action: 'skill_staged', changes: [{ action: 'skill_staged', skill_name: 'test-recipe', pending_id: 'pending-1' }] };
    const view = render(<SelfImprovementBadge review={stale} />);
    fireEvent.click(screen.getByRole('button', { name: /Self-improvement review/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Approve proposal' }));
    expect(await screen.findByText('# Approved')).toBeTruthy();
    view.rerender(<SelfImprovementBadge review={structuredClone(stale)} />);
    expect(screen.getByText('# Approved')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Approve proposal' })).toBeNull();
    view.unmount();
    render(<SelfImprovementBadge review={stale} />);
    fireEvent.click(screen.getByRole('button', { name: /Self-improvement review/ }));
    expect((await screen.findByRole('alert')).textContent).toContain('no longer available');
    expect(screen.queryByRole('button', { name: 'Approve proposal' })).toBeNull();
    expect(screen.queryByText('# Approved')).toBeNull();
    expect(screen.getByRole('button', { name: /Self-improvement review/ }).textContent).toContain('staged');
  });

  it('reads distinct files for changes to the same skill and ignores a stale target response', async () => {
    let finishFirst!: (content: string) => void;
    vi.mocked(invoke)
      .mockImplementationOnce(() => new Promise<string>((resolve) => { finishFirst = resolve; }))
      .mockResolvedValueOnce('# Supporting file');
    render(<SelfImprovementBadge review={{ ...created, changes: [
      { action: 'skill_updated', skill_name: 'test-recipe', file_path: 'D:/global/test-recipe/SKILL.md' },
      { action: 'skill_updated', skill_name: 'test-recipe', file_path: 'D:/global/test-recipe/scripts/check.ts' },
    ] }} />);
    fireEvent.click(screen.getByRole('button', { name: /Self-improvement review/ }));
    expect(invoke).toHaveBeenLastCalledWith('read_reflection_skill', { runId: 'run-1', sessionId: 'session-1', skillName: 'test-recipe', filePath: 'D:/global/test-recipe/SKILL.md' });
    fireEvent.click(screen.getAllByRole('button', { name: /test-recipe .* updated/ })[1]);
    expect(await screen.findByText('# Supporting file')).toBeTruthy();
    expect(invoke).toHaveBeenLastCalledWith('read_reflection_skill', { runId: 'run-1', sessionId: 'session-1', skillName: 'test-recipe', filePath: 'D:/global/test-recipe/scripts/check.ts' });
    await act(async () => { finishFirst('# Stale skill'); });
    expect(screen.getByText('# Supporting file')).toBeTruthy();
    expect(screen.queryByText('# Stale skill')).toBeNull();
  });


  it('loads a persisted staged proposal as a diff, guards duplicate approval, and remembers approval on reopen', async () => {
    let approve!: (change: unknown) => void;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'list_review_proposals') return [{ id: 'pending-1', skill_name: 'test-recipe', file_path: 'D:/global/test-recipe/SKILL.md' }];
      if (command === 'review_proposal_diff') return { id: 'pending-1', skill_name: 'test-recipe', file_path: 'D:/global/test-recipe/SKILL.md', before: 'old', after: 'new', diff: '-old\n+new' };
      if (command === 'approve_review_proposal') return new Promise((resolve) => { approve = resolve; });
      if (command === 'read_reflection_skill') return '# Approved skill';
    });
    render(<SelfImprovementBadge review={{ ...created, action: 'skill_staged', changes: [{ action: 'skill_staged', skill_name: 'test-recipe', pending_id: 'pending-1' }] }} />);
    const trigger = screen.getByRole('button', { name: /Self-improvement review/ });
    expect(trigger.textContent).toContain('staged');
    expect(trigger.textContent).not.toMatch(/created|updated/);
    fireEvent.click(trigger);
    expect(await screen.findByText('-old +new')).toBeTruthy();
    expect(invoke).toHaveBeenCalledWith('list_review_proposals', { sessionId: 'session-1' });
    expect(invoke).toHaveBeenCalledWith('review_proposal_diff', { sessionId: 'session-1', id: 'pending-1' });
    const button = screen.getByRole('button', { name: 'Approve proposal' });
    fireEvent.click(button);
    fireEvent.click(button);
    expect((screen.getByRole('button', { name: 'Reject proposal' }) as HTMLButtonElement).disabled).toBe(true);
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'approve_review_proposal')).toHaveLength(1);
    await act(async () => { approve({ action: 'skill_updated', skill_name: 'test-recipe', file_path: 'D:/global/test-recipe/SKILL.md' }); });
    expect(await screen.findByText('# Approved skill')).toBeTruthy();
    expect(invoke).toHaveBeenLastCalledWith('read_reflection_skill', { runId: 'run-1', sessionId: 'session-1', skillName: 'test-recipe', filePath: 'D:/global/test-recipe/SKILL.md' });
    expect(screen.queryByRole('button', { name: 'Approve proposal' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Close skill inspector' }));
    fireEvent.click(trigger);
    expect(await screen.findByText('# Approved skill')).toBeTruthy();
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'list_review_proposals')).toHaveLength(1);
  });

  it('rejects a listed proposal without a pending id, guards both actions, and remembers rejection on reopen', async () => {
    let reject!: () => void;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'list_review_proposals') return [{ id: 'listed-id', skill_name: 'test-recipe', file_path: '/skill/SKILL.md' }];
      if (command === 'review_proposal_diff') return { id: 'listed-id', skill_name: 'test-recipe', file_path: '/skill/SKILL.md', before: '', after: '<script>unsafe</script>', diff: '+<script>unsafe</script>' };
      if (command === 'reject_review_proposal') return new Promise<void>((resolve) => { reject = resolve; });
      throw new Error(`Unexpected command: ${command}`);
    });
    render(<SelfImprovementBadge review={{ ...created, action: 'skill_staged' }} />);
    const trigger = screen.getByRole('button', { name: /Self-improvement review/ });
    expect(trigger.textContent).toContain('staged');
    expect(trigger.textContent).not.toMatch(/created|updated/);
    fireEvent.click(trigger);
    expect(await screen.findByText('+<script>unsafe</script>')).toBeTruthy();
    expect(screen.getByRole('dialog').querySelector('script')).toBeNull();
    const button = screen.getByRole('button', { name: 'Reject proposal' });
    fireEvent.click(button);
    fireEvent.click(button);
    fireEvent.click(screen.getByRole('button', { name: 'Approve proposal' }));
    expect(invoke).toHaveBeenCalledWith('reject_review_proposal', { sessionId: 'session-1', id: 'listed-id' });
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'reject_review_proposal')).toHaveLength(1);
    expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'approve_review_proposal')).toBe(false);
    await act(async () => { reject(); });
    expect(await screen.findByText('Proposal rejected. No skill changes were applied.')).toBeTruthy();
    expect(trigger.textContent).toContain('rejected');
    fireEvent.click(screen.getByRole('button', { name: 'Close skill inspector' }));
    fireEvent.click(trigger);
    expect(await screen.findByText('Proposal rejected. No skill changes were applied.')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Approve proposal' })).toBeNull();
    expect(invoke).toHaveBeenCalledTimes(3);
  });

  it.each(['list_review_proposals', 'review_proposal_diff', 'approve_review_proposal', 'reject_review_proposal'] as const)('ignores a stale %s response after retargeting the review', async (pendingCommand) => {
    let finish!: (value: unknown) => void;
    const proposal = { id: 'pending-1', skill_name: 'test-recipe', file_path: '/skill/SKILL.md', before: 'old', after: 'new', diff: '-old\n+new' };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === pendingCommand) return new Promise((resolve) => { finish = resolve; });
      if (command === 'list_review_proposals') return [proposal];
      if (command === 'review_proposal_diff') return proposal;
      if (command === 'read_reflection_skill') return '# Current session';
      throw new Error(`Unexpected command: ${command}`);
    });
    const { rerender } = render(<SelfImprovementBadge review={{ ...created, action: 'skill_staged', changes: [{ action: 'skill_staged', skill_name: 'test-recipe', pending_id: 'pending-1' }] }} />);
    fireEvent.click(screen.getByRole('button', { name: /Self-improvement review/ }));
    if (pendingCommand === 'approve_review_proposal' || pendingCommand === 'reject_review_proposal') {
      fireEvent.click(await screen.findByRole('button', { name: pendingCommand === 'approve_review_proposal' ? 'Approve proposal' : 'Reject proposal' }));
    }
    await waitFor(() => expect(finish).toBeTypeOf('function'));
    rerender(<SelfImprovementBadge review={{ ...created, run_id: 'run-2', session_id: 'session-2', skill_name: 'current' }} />);
    expect(await screen.findByText('# Current session')).toBeTruthy();
    const callCount = vi.mocked(invoke).mock.calls.length;
    await act(async () => {
      finish(pendingCommand === 'list_review_proposals' ? [proposal] : pendingCommand === 'review_proposal_diff' ? proposal : { action: 'skill_updated', skill_name: 'test-recipe' });
    });
    expect(screen.getByText('# Current session')).toBeTruthy();
    expect(screen.queryByRole('alert')).toBeNull();
    expect(screen.queryByText('Proposal rejected. No skill changes were applied.')).toBeNull();
    expect(invoke).toHaveBeenCalledTimes(callCount);
    fireEvent.click(screen.getByRole('button', { name: 'Close skill inspector' }));
    fireEvent.click(screen.getByRole('button', { name: /Self-improvement review/ }));
    expect(await screen.findByText('# Current session')).toBeTruthy();
    expect(invoke).toHaveBeenLastCalledWith('read_reflection_skill', { runId: 'run-2', sessionId: 'session-2', skillName: 'current' });
  });

  it('does not send a duplicate decision after switching away from a pending proposal and back', async () => {
    let finish!: (value: unknown) => void;
    const proposal = { id: 'pending-1', skill_name: 'test-recipe', file_path: '/skill/SKILL.md', before: '', after: 'new', diff: '+new' };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'list_review_proposals') return [proposal];
      if (command === 'review_proposal_diff') return proposal;
      if (command === 'read_reflection_skill') return '# Other skill';
      if (command === 'approve_review_proposal') return new Promise((resolve) => { finish = resolve; });
    });
    render(<SelfImprovementBadge review={{ ...created, action: 'skill_staged', changes: [
      { action: 'skill_staged', skill_name: 'test-recipe', pending_id: 'pending-1' },
      { action: 'skill_created', skill_name: 'other' },
    ] }} />);
    fireEvent.click(screen.getByRole('button', { name: /Self-improvement review/ }));
    fireEvent.click(await screen.findByRole('button', { name: 'Approve proposal' }));
    fireEvent.click(screen.getByRole('button', { name: /other .* created/ }));
    expect(await screen.findByText('# Other skill')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: /test-recipe .* staged/ }));
    expect(await screen.findByText('+new')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Approve proposal' }));
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === 'approve_review_proposal')).toHaveLength(1);
    await act(async () => { finish({ action: 'skill_created', skill_name: 'test-recipe' }); });
    await waitFor(() => expect((screen.getByRole('button', { name: 'Reject proposal' }) as HTMLButtonElement).disabled).toBe(false));
  });

  it.each(['approve', 'reject'] as const)('blocks duplicate %s after closing and reopening, and ignores the dismissed failure', async (decision) => {
    let fail!: (reason: string) => void;
    const proposal = { id: 'pending-1', skill_name: 'test-recipe', file_path: '/skill/SKILL.md', before: '', after: 'new', diff: '+new' };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'list_review_proposals') return [proposal];
      if (command === 'review_proposal_diff') return proposal;
      if (command === `${decision}_review_proposal`) return new Promise((_resolve, reject) => { fail = reject; });
    });
    render(<SelfImprovementBadge review={{ ...created, action: 'skill_staged' }} />);
    const trigger = screen.getByRole('button', { name: /Self-improvement review/ });
    const decisionLabel = decision === 'approve' ? 'Approve proposal' : 'Reject proposal';
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole('button', { name: decisionLabel }));
    fireEvent.click(screen.getByRole('button', { name: 'Close skill inspector' }));
    fireEvent.click(trigger);
    const button = await screen.findByRole('button', { name: decisionLabel });
    expect((button as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(button);
    expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === `${decision}_review_proposal`)).toHaveLength(1);
    await act(async () => { fail('Dismissed failure'); });
    expect(screen.queryByRole('alert')).toBeNull();
    expect((button as HTMLButtonElement).disabled).toBe(false);
    expect(trigger.textContent).toContain('staged');
  });

  it('keeps a failed approval staged and allows a scoped retry', async () => {
    const proposal = { id: 'pending-1', skill_name: 'test-recipe', file_path: '/skill/SKILL.md', before: '', after: 'new', diff: '+new' };
    let approvals = 0;
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'list_review_proposals') return [proposal];
      if (command === 'review_proposal_diff') return proposal;
      if (command === 'approve_review_proposal') {
        if (++approvals === 1) throw 'Approval failed';
        return { action: 'skill_created', skill_name: 'test-recipe' };
      }
      if (command === 'read_reflection_skill') return '# Approved after retry';
    });
    render(<SelfImprovementBadge review={{ ...created, action: 'skill_staged' }} />);
    const trigger = screen.getByRole('button', { name: /Self-improvement review/ });
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole('button', { name: 'Approve proposal' }));
    expect((await screen.findByRole('alert')).textContent).toContain('Approval failed');
    expect(trigger.textContent).toContain('staged');
    expect((screen.getByRole('button', { name: 'Approve proposal' }) as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(await screen.findByText('+new')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Approve proposal' }));
    expect(await screen.findByText('# Approved after retry')).toBeTruthy();
    expect(approvals).toBe(2);
    expect(trigger.textContent).toContain('created');
    expect(trigger.textContent).not.toContain('staged');
  });

  it.each(['missing', 'ambiguous', 'mismatched'] as const)('does not allow decisions on a %s proposal', async (kind) => {
    const proposal = { id: 'pending-1', skill_name: 'test-recipe', file_path: '/skill/SKILL.md', before: '', after: 'new', diff: '+new' };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'list_review_proposals') return kind === 'missing' ? [] : kind === 'ambiguous' ? [proposal, { ...proposal, id: 'pending-2' }] : [proposal];
      if (command === 'review_proposal_diff') return { ...proposal, id: 'wrong-id' };
      throw new Error(`Unexpected command: ${command}`);
    });
    render(<SelfImprovementBadge review={{ ...created, action: 'skill_staged' }} />);
    fireEvent.click(screen.getByRole('button', { name: /Self-improvement review/ }));
    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(screen.queryByRole('button', { name: 'Approve proposal' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Reject proposal' })).toBeNull();
    expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'read_reflection_skill')).toBe(false);
  });

  it('shows a multi-change summary and lets the inspector select every scoped skill', async () => {
    vi.mocked(invoke).mockImplementation(async (_command, args) => `# ${(args as { skillName: string }).skillName}`);
    render(<SelfImprovementBadge review={{ ...created, skill_name: undefined, summary: 'Two reusable lessons', changes: [
      { action: 'skill_created', skill_name: 'first', file_path: 'D:/global/first/SKILL.md', summary: 'First lesson' },
      { action: 'skill_updated', skill_name: 'second', skill_path: 'D:/global/second/SKILL.md' },
    ] }} />);
    fireEvent.click(screen.getByRole('button', { name: 'Self-improvement review · 2 changes · Two reusable lessons' }));
    expect(await screen.findByText('# first')).toBeTruthy();
    expect(screen.getByText('D:/global/first/SKILL.md')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'second · updated' }));
    expect(await screen.findByText('# second')).toBeTruthy();
    expect(invoke).toHaveBeenLastCalledWith('read_reflection_skill', { runId: 'run-1', sessionId: 'session-1', skillName: 'second' });
  });
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
