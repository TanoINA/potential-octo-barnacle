import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { SelectedSkillChip } from './SelectedSkillChip';
import type { SkillInfo } from '../types/agent';

const skill: SkillInfo = {
  name: 'ponytail',
  description: 'Small complete changes',
  path: 'D:/project/.aether/skills/ponytail/SKILL.md',
  source: 'D:/project/.aether/skills',
};

function setup(metadata: SkillInfo | undefined = skill) {
  const onRemove = vi.fn();
  const view = render(<SelectedSkillChip name="ponytail" skill={metadata} onRemove={onRemove} />);
  return { ...view, onRemove, toggle: screen.getByRole('button', { name: 'Preview skill ponytail' }) };
}

describe('SelectedSkillChip', () => {
  it('toggles a named non-modal metadata preview outside the chip container', () => {
    const { toggle, container, onRemove } = setup();
    expect(toggle.getAttribute('title')).toBe(skill.description);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(toggle.getAttribute('aria-haspopup')).toBe('dialog');
    expect(screen.queryByRole('dialog')).toBeNull();
    fireEvent.click(toggle);
    const preview = screen.getByRole('dialog', { name: 'Skill preview: ponytail' });
    expect(container.contains(preview)).toBe(false);
    expect(preview.getAttribute('aria-modal')).not.toBe('true');
    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(toggle.getAttribute('aria-controls')).toBe(preview.id);
    expect(within(preview).getByText(skill.description)).toBeTruthy();
    expect(within(preview).getByText(skill.source)).toBeTruthy();
    expect(within(preview).getByText(skill.path)).toBeTruthy();
    expect(document.activeElement).toBe(preview);
    fireEvent.click(toggle);
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(onRemove).not.toHaveBeenCalled();
  });

  it('closes on Escape, consumes the key, and restores trigger focus', () => {
    const { toggle } = setup();
    const escaped = vi.fn();
    document.addEventListener('keydown', escaped);
    try {
      fireEvent.click(toggle);
      fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
      expect(screen.queryByRole('dialog')).toBeNull();
      expect(document.activeElement).toBe(toggle);
      expect(toggle.getAttribute('aria-expanded')).toBe('false');
      expect(escaped).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener('keydown', escaped);
    }
  });

  it('keeps inside interactions open and dismisses on outside pointer or keyboard focus', () => {
    const { toggle } = setup();
    fireEvent.click(toggle);
    fireEvent.pointerDown(screen.getByText(skill.path));
    expect(screen.getByRole('dialog')).toBeTruthy();
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole('dialog')).toBeNull();
    fireEvent.click(toggle);
    const outside = document.createElement('button');
    document.body.append(outside);
    try {
      act(() => outside.focus());
      expect(screen.queryByRole('dialog')).toBeNull();
      expect(document.activeElement).toBe(outside);
    } finally {
      outside.remove();
    }
  });

  it('has an explicit close button which returns focus to the toggle', () => {
    const { toggle } = setup();
    fireEvent.click(toggle);
    fireEvent.click(screen.getByRole('button', { name: 'Close skill preview' }));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(toggle);
  });

  it('removes without opening or submitting, including while the preview is open', () => {
    const { toggle, onRemove } = setup();
    const remove = screen.getByRole('button', { name: 'Remove skill ponytail' });
    expect(remove.getAttribute('type')).toBe('button');
    expect(toggle.getAttribute('type')).toBe('button');
    fireEvent.click(remove);
    expect(onRemove).toHaveBeenCalledOnce();
    expect(screen.queryByRole('dialog')).toBeNull();
    fireEvent.click(toggle);
    fireEvent.click(remove);
    expect(onRemove).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it.each([undefined, { ...skill, description: ' ', path: '', source: ' ' }])('handles unavailable or empty metadata (%s)', (metadata) => {
    render(<SelectedSkillChip name="missing" skill={metadata} onRemove={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Preview skill missing' }));
    const preview = screen.getByRole('dialog', { name: 'Skill preview: missing' });
    expect(within(preview).getByText('No description')).toBeTruthy();
    expect(within(preview).getByText('Source unavailable')).toBeTruthy();
    expect(within(preview).getByText('Path unavailable')).toBeTruthy();
  });

  it('renders metadata as text rather than markup', () => {
    const unsafe = '<img src=x onerror=alert(1)>';
    setup({ ...skill, description: unsafe });
    fireEvent.click(screen.getByRole('button', { name: 'Preview skill ponytail' }));
    const preview = screen.getByRole('dialog');
    expect(within(preview).getByText(unsafe)).toBeTruthy();
    expect(preview.querySelector('img')).toBeNull();
  });

  it('anchors above the chip, follows scrolling, and clamps inside the viewport', () => {
    const { toggle } = setup();
    const bounds = (left: number, top: number, width: number, height: number) => ({
      left, top, width, height, right: left + width, bottom: top + height,
    }) as DOMRect;
    let anchor = bounds(100, 400, 100, 24);
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      return this.getAttribute('role') === 'dialog' ? bounds(0, 0, 352, 200) : anchor;
    });
    fireEvent.click(toggle);
    const preview = screen.getByRole('dialog');
    expect(preview.style.left).toBe('100px');
    expect(preview.style.top).toBe('194px');
    anchor = bounds(100, 10, 100, 24);
    fireEvent.scroll(document);
    expect(preview.style.top).toBe('40px');
    anchor = bounds(window.innerWidth - 10, 400, 100, 24);
    fireEvent.resize(window);
    expect(preview.style.left).toBe(`${window.innerWidth - 352 - 8}px`);
  });

  it('gives each chip unique preview controls and closes the old preview when another is opened', () => {
    render(<><SelectedSkillChip name="one" skill={undefined} onRemove={vi.fn()} /><SelectedSkillChip name="two" skill={undefined} onRemove={vi.fn()} /></>);
    const first = screen.getByRole('button', { name: 'Preview skill one' });
    const second = screen.getByRole('button', { name: 'Preview skill two' });
    fireEvent.click(first);
    const firstId = screen.getByRole('dialog').id;
    fireEvent.pointerDown(second);
    fireEvent.click(second);
    expect(screen.getAllByRole('dialog')).toHaveLength(1);
    expect(screen.getByRole('dialog', { name: 'Skill preview: two' }).id).not.toBe(firstId);
    expect(first.getAttribute('aria-expanded')).toBe('false');
  });
});
