import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { InteractivePlanCard } from './InteractivePlanCard';

describe('InteractivePlanCard', () => {
  const samplePlan = `
- [x] Step 1: Analyze directory layout
- [-] Step 2: Implement core algorithm
- [ ] Step 3: Run regression tests
`;

  it('exposes focusable native checkbox buttons with stable names and checked states', () => {
    const { rerender } = render(<InteractivePlanCard rawPlan={samplePlan} />);
    const done = screen.getByRole('checkbox', { name: 'Step 1: Analyze directory layout' });
    const active = screen.getByRole('checkbox', { name: 'Step 2: Implement core algorithm' });
    const pending = screen.getByRole('checkbox', { name: 'Step 3: Run regression tests' });

    for (const row of [done, active, pending]) {
      expect(row.tagName).toBe('BUTTON');
      expect(row.getAttribute('type')).toBe('button');
      expect(row.tabIndex).toBe(0);
    }
    expect(done.getAttribute('aria-checked')).toBe('true');
    expect(active.getAttribute('aria-checked')).toBe('false');
    expect(pending.getAttribute('aria-checked')).toBe('false');
    pending.focus();
    expect(document.activeElement).toBe(pending);
    // jsdom does not synthesize native button clicks from Space/Enter.
    fireEvent.click(pending);
    expect(pending.getAttribute('aria-checked')).toBe('true');
    expect(pending.getAttribute('aria-label')).toBe('Step 3: Run regression tests');

    rerender(<InteractivePlanCard rawPlan={'- [ ] Step 3: Run regression tests\n- [x] Step 1: Analyze directory layout\n- [-] Step 2: Implement core algorithm'} />);
    expect(screen.getByRole('checkbox', { name: 'Step 3: Run regression tests' })).toBe(pending);
    expect(pending.getAttribute('aria-checked')).toBe('true');
    expect(document.activeElement).toBe(pending);
    fireEvent.click(pending);
    expect(pending.getAttribute('aria-checked')).toBe('false');
    expect(pending.getAttribute('aria-label')).toBe('Step 3: Run regression tests');
  });

  it('reports bounded progress through toggles, source changes, and an empty plan', () => {
    const { rerender } = render(<InteractivePlanCard rawPlan={samplePlan} />);
    const progress = screen.getByRole('progressbar', { name: 'Plan progress' });
    expect(progress.getAttribute('aria-valuemin')).toBe('0');
    expect(progress.getAttribute('aria-valuemax')).toBe('100');
    expect(progress.getAttribute('aria-valuenow')).toBe('33');
    expect((progress.firstElementChild as HTMLElement).style.width).toBe('33%');

    fireEvent.click(screen.getByText('Step 3: Run regression tests'));
    expect(progress.getAttribute('aria-valuenow')).toBe('67');
    rerender(<InteractivePlanCard rawPlan={'- [x] Finished'} />);
    expect(progress.getAttribute('aria-valuenow')).toBe('100');
    rerender(<InteractivePlanCard rawPlan={''} />);
    expect(progress.getAttribute('aria-valuenow')).toBe('0');
    expect((progress.firstElementChild as HTMLElement).style.width).toBe('0%');
  });

  it('uses a solid blue progress fill and a quiet card without gradients, blur, or pulse', () => {
    render(<InteractivePlanCard rawPlan={samplePlan} />);
    const card = screen.getByTestId('interactive-plan-card');
    expect(card.className).toContain('rounded-lg');
    expect(card.className).toContain('shadow-sm');
    expect(card.querySelector('[class*="gradient"], [class*="blur"], [class*="animate-pulse"]')).toBeNull();
    expect(card.className).not.toContain('blur');
    const fill = screen.getByRole('progressbar', { name: 'Plan progress' }).firstElementChild!;
    expect(fill.className).toContain('bg-[#58a6ff]');
    expect(fill.className).toContain('transition-[width]');
    expect(fill.className).toContain('duration-[160ms]');
    expect(fill.className).toContain('ease-out');
    expect(fill.className).not.toContain('transition-all');
  });

  it('renders plan items with completion counts and progress percent', () => {
    render(<InteractivePlanCard rawPlan={samplePlan} />);
    expect(screen.getByText('Execution Plan')).toBeTruthy();
    expect(screen.getByText('1/3 completed')).toBeTruthy();
    expect(screen.getByText('33%')).toBeTruthy();
    expect(screen.getByText('Step 1: Analyze directory layout')).toBeTruthy();
    expect(screen.getByText('Step 2: Implement core algorithm')).toBeTruthy();
    expect(screen.getByText('Step 3: Run regression tests')).toBeTruthy();
  });

  it('allows toggling plan item status on click', () => {
    render(<InteractivePlanCard rawPlan={samplePlan} />);
    const pendingStep = screen.getByText('Step 3: Run regression tests');

    fireEvent.click(pendingStep);
    // After clicking pending item, completed count becomes 2/3 (67%)
    expect(screen.getByText('2/3 completed')).toBeTruthy();
    expect(screen.getByText('67%')).toBeTruthy();

    fireEvent.click(pendingStep);
    // After clicking again, reverts to 1/3
    expect(screen.getByText('1/3 completed')).toBeTruthy();
  });

  it('parses numbered plan items with checkboxes correctly', () => {
    const numberedPlan = `
1. [x] First numbered task
2. [-] Second in progress task
3. [ ] Third pending task
`;
    render(<InteractivePlanCard rawPlan={numberedPlan} />);
    expect(screen.getByText('1/3 completed')).toBeTruthy();
    expect(screen.getByText('First numbered task')).toBeTruthy();
    expect(screen.getByText('Second in progress task')).toBeTruthy();
    expect(screen.getByText('Third pending task')).toBeTruthy();
  });

  it('reconciles streamed partial text and appended steps without losing unchanged toggles', () => {
    const { rerender } = render(<InteractivePlanCard rawPlan={'- [ ] Inspect files\n- [ ] Implement rec'} />);
    fireEvent.click(screen.getByText('Inspect files'));

    rerender(<InteractivePlanCard rawPlan={'- [ ] Inspect files\n- [ ] Implement reconciliation\n- [x] Run tests'} />);

    expect(screen.queryByText('Implement rec')).toBeNull();
    expect(screen.getByText('Implement reconciliation')).toBeTruthy();
    expect(screen.getByText('Inspect files').className).toContain('line-through');
    expect(screen.getByText('Run tests').className).toContain('line-through');
    expect(screen.getByText('2/3 completed')).toBeTruthy();
  });

  it('lets changed source statuses override manual toggles in either direction', () => {
    const { rerender } = render(<InteractivePlanCard rawPlan={'- [ ] Build'} />);
    fireEvent.click(screen.getByText('Build'));
    rerender(<InteractivePlanCard rawPlan={'- [x] Build'} />);
    expect(screen.getByText('1/1 completed')).toBeTruthy();
    rerender(<InteractivePlanCard rawPlan={'- [ ] Build'} />);
    expect(screen.getByText('0/1 completed')).toBeTruthy();

    fireEvent.click(screen.getByText('Build'));
    rerender(<InteractivePlanCard rawPlan={'- [-] Build'} />);
    expect(screen.getByText('Build').className).toContain('text-[#f0883e]');
    rerender(<InteractivePlanCard rawPlan={'- [X] Build'} />);
    fireEvent.click(screen.getByText('Build'));
    rerender(<InteractivePlanCard rawPlan={'- [X] Build\n- [ ] Ship'} />);
    expect(screen.getByText('0/2 completed')).toBeTruthy();
    rerender(<InteractivePlanCard rawPlan={'- [~] Build\n- [ ] Ship'} />);
    expect(screen.getByText('Build').className).toContain('text-[#f0883e]');
  });

  it('keeps duplicate occurrences independent when steps move or are corrected and removed', () => {
    const { rerender } = render(<InteractivePlanCard rawPlan={'- [ ] Repeat\n- [ ] Repeat\n- [ ] Keep'} />);
    fireEvent.click(screen.getAllByText('Repeat')[1]);
    fireEvent.click(screen.getByText('Keep'));
    rerender(<InteractivePlanCard rawPlan={'- [ ] Keep\n- [ ] Repeat\n- [ ] Repeat\n- [ ] New'} />);
    const duplicates = screen.getAllByText('Repeat');
    expect(duplicates[0].className).not.toContain('line-through');
    expect(duplicates[1].className).toContain('line-through');
    expect(screen.getByText('Keep').className).toContain('line-through');
    expect(screen.getByText('2/4 completed')).toBeTruthy();

    rerender(<InteractivePlanCard rawPlan={'- [ ] Corrected\n- [ ] New'} />);
    expect(screen.queryByText('Repeat')).toBeNull();
    expect(screen.queryByText('Keep')).toBeNull();
    expect(screen.getByText('0/2 completed')).toBeTruthy();
    rerender(<InteractivePlanCard rawPlan={'- [ ] Keep'} />);
    expect(screen.getByText('0/1 completed')).toBeTruthy();
  });

});
