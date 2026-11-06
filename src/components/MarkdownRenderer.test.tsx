import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { MarkdownRenderer } from './MarkdownRenderer';

describe('MarkdownRenderer', () => {
  it('preserves indentation inside fenced code', () => {
    const { container } = render(<MarkdownRenderer content={'```python\nif ready:\n    print("ok")\n```'} />);
    const code = container.querySelector('pre code');
    expect(code?.textContent).toContain('\n    print("ok")');
  });

  it('removes prose emoji without altering code or collapsing spaces', () => {
    const { container } = render(<MarkdownRenderer content={'Hello 😄\n\n```text\n😄  keep    spaces\n```'} />);
    expect(container.textContent).toContain('Hello');
    expect(container.textContent).not.toContain('Hello 😄');
    expect(container.querySelector('pre code')?.textContent).toBe('😄  keep    spaces');
  });
});
