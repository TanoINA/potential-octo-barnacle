import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { SkillPicker } from './SkillPicker';

const skills = [
  { name: 'ponytail', description: 'Small complete changes', path: 'D:/project/.aether/skills/ponytail/SKILL.md', source: 'D:/project/.aether/skills' },
  { name: 'debugging', description: 'Find the root cause', path: 'C:/skills/debugging/SKILL.md', source: 'C:/skills' },
];
describe('SkillPicker', () => {
  it('searches descriptions and toggles selection without submitting a prompt', () => {
    const toggle = vi.fn();
    const close = vi.fn();
    render(<SkillPicker skills={skills} selected={['ponytail']} onToggle={toggle} onClose={close} />);
    const search = screen.getByRole('searchbox', { name: 'Search skills' });
    expect(document.activeElement).toBe(search);
    expect(screen.getByRole('button', { name: /ponytail/ }).getAttribute('aria-pressed')).toBe('true');
    fireEvent.change(search, { target: { value: 'root cause' } });
    expect(screen.queryByRole('button', { name: /ponytail/ })).toBeNull();
    fireEvent.keyDown(search, { key: 'ArrowDown' });
    const result = screen.getByRole('button', { name: /debugging/ });
    expect(document.activeElement).toBe(result);
    fireEvent.click(result);
    expect(toggle).toHaveBeenCalledWith('debugging');
    expect(close).not.toHaveBeenCalled();
    fireEvent.keyDown(result, { key: 'Escape' });
    expect(close).toHaveBeenCalledOnce();
  });
  it('allows removal but prevents a sixth selection', () => {
    render(<SkillPicker skills={skills} selected={['ponytail', 'two', 'three', 'four', 'five']} onToggle={vi.fn()} onClose={vi.fn()} />);
    expect((screen.getByRole('button', { name: /ponytail/ }) as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByRole('button', { name: /debugging/ }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText('5/5 selected')).toBeTruthy();
  });
  it('distinguishes loading, failure, and an empty search', () => {
    const props = { skills, selected: [], onToggle: vi.fn(), onClose: vi.fn() };
    const { rerender } = render(<SkillPicker {...props} loading />);
    expect(screen.getByRole('status').textContent).toContain('Loading');
    rerender(<SkillPicker {...props} error="Cannot read skill catalog" />);
    expect(screen.getByRole('alert').textContent).toBe('Cannot read skill catalog');
    rerender(<SkillPicker {...props} />);
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'missing skill' } });
    expect(screen.getByText('No matching skills')).toBeTruthy();
  });
});
