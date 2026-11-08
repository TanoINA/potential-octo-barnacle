import { fireEvent, render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { TerminalOutput } from './TerminalOutput';

describe('TerminalOutput sticky tail', () => {
  it('follows a large appended chunk only while the user remains at the bottom', () => {
    const { container, rerender } = render(<TerminalOutput output="initial" />);
    const log = container.querySelector('[data-selectable-text]') as HTMLDivElement;
    let height = 1000;
    Object.defineProperties(log, {
      scrollHeight: { get: () => height },
      clientHeight: { value: 200 },
    });
    log.scrollTop = 800;
    fireEvent.scroll(log);
    height = 1600;
    rerender(<TerminalOutput output={'initial\n' + 'next line\n'.repeat(30)} />);
    // jsdom does not clamp scrollTop; assert the intended tail assignment.
    expect(log.scrollTop).toBe(1600);

    log.scrollTop = 300;
    fireEvent.scroll(log);
    height = 2200;
    rerender(<TerminalOutput output={'initial\n' + 'next line\n'.repeat(60)} />);
    expect(log.scrollTop).toBe(300);

    log.scrollTop = 2000;
    fireEvent.scroll(log);
    height = 2800;
    rerender(<TerminalOutput output={'initial\n' + 'next line\n'.repeat(90)} />);
    expect(log.scrollTop).toBe(2800);
  });
});
