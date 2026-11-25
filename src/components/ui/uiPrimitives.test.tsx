import { useState } from 'react';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { SegmentedControl } from './index';

afterEach(() => {
  cleanup();
});

describe('UI primitives', () => {
  it('supports click and arrow/Home/End navigation in SegmentedControl', () => {
    function Harness() {
      const [val, setVal] = useState<'a' | 'b' | 'c'>('a');
      return (
        <SegmentedControl
          ariaLabel="Sample options"
          value={val}
          onChange={setVal}
          options={[
            { value: 'a', label: 'First' },
            { value: 'b', label: 'Second' },
            { value: 'c', label: 'Third' },
          ]}
        />
      );
    }

    render(<Harness />);
    const first = screen.getByRole('radio', { name: 'First' });
    const second = screen.getByRole('radio', { name: 'Second' });
    const third = screen.getByRole('radio', { name: 'Third' });

    expect(first.getAttribute('aria-checked')).toBe('true');
    first.focus();

    fireEvent.keyDown(first, { key: 'ArrowRight' });
    expect(document.activeElement).toBe(second);
    expect(second.getAttribute('aria-checked')).toBe('true');

    fireEvent.keyDown(second, { key: 'End' });
    expect(document.activeElement).toBe(third);
    expect(third.getAttribute('aria-checked')).toBe('true');

    fireEvent.keyDown(third, { key: 'Home' });
    expect(document.activeElement).toBe(first);
    expect(first.getAttribute('aria-checked')).toBe('true');

    fireEvent.keyDown(first, { key: 'ArrowLeft' });
    expect(document.activeElement).toBe(third);
    expect(third.getAttribute('aria-checked')).toBe('true');

    fireEvent.click(second);
    expect(second.getAttribute('aria-checked')).toBe('true');
  });
});
