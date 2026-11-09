import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { MarkdownRenderer } from './MarkdownRenderer';

const sample = String.raw`### Perhitungan

\[
\begin{aligned}
T(n) &= O(1)+O(1) \\
     &\quad + O(n)\times\bigl(O(n)\times O(1)\bigr) \\
     &= O(n^2)
\end{aligned}
\]

Kompleksitas akhirnya \(O(n^2)\).`;

describe('Markdown academic math', () => {
  it('renders the screenshot-style aligned calculation and inline LaTeX as accessible math', () => {
    const { container } = render(<MarkdownRenderer content={sample} />);
    expect(container.querySelectorAll('.katex-display')).toHaveLength(1);
    expect(container.querySelectorAll('.katex')).toHaveLength(2);
    expect(container.querySelector('math[display="block"]')).toBeTruthy();
    expect(container.querySelector('annotation[encoding="application/x-tex"]')?.textContent).toContain('\\begin{aligned}');
    expect(container.querySelector('.katex-html')?.textContent).not.toContain('\\begin');
    expect(container.querySelector('h3')?.textContent).toBe('Perhitungan');
  });

  it('supports standard dollar delimiters, fractions, matrices and math inside tables', () => {
    const content = String.raw`Inline $\frac{a}{b}$.

$$
\begin{bmatrix}1 & 2 \\ 3 & 4\end{bmatrix}
$$

| Rumus | Makna |
| --- | --- |
| $n^2$ | Kuadrat |`;
    const { container } = render(<MarkdownRenderer content={content} />);
    expect(container.querySelectorAll('.katex')).toHaveLength(3);
    expect(container.querySelector('table .katex')).toBeTruthy();
    expect(container.querySelector('.katex-display .mtable')).toBeTruthy();
  });

  it('does not interpret math delimiters in inline, fenced or indented code', () => {
    const inline = String.raw`\(literal\) and $x$`;
    const block = String.raw`\[literal\]
$$not_math$$`;
    const content = '`' + inline + '`\n\n```latex\n' + block + '\n```\n\n    \\(indented\\)';
    const { container } = render(<MarkdownRenderer content={content} />);
    expect(container.querySelectorAll('.katex')).toHaveLength(0);
    expect(container.querySelector('p code')?.textContent).toBe(inline);
    expect(container.querySelector('pre code')?.textContent).toBe(block);
    expect(container.textContent).toContain('\\(indented\\)');
  });

  it('does not let literal openers in code swallow a later equation', () => {
    const { container } = render(<MarkdownRenderer content={'Literal `' + String.raw`\(` + '`. Actual formula: ' + String.raw`\(x^2\).`} />);
    expect(container.querySelectorAll('.katex')).toHaveLength(1);
    expect(container.querySelector('p code')?.textContent).toBe(String.raw`\(`);
  });

  it('keeps a math-labelled code fence literal with its copy control', () => {
    const { container, getByTitle } = render(<MarkdownRenderer content={'```math\nx^2\n```'} />);
    expect(container.querySelectorAll('.katex')).toHaveLength(0);
    expect(container.querySelector('pre code')?.textContent).toBe('x^2');
    expect(getByTitle('Copy code')).toBeTruthy();
  });

  it('keeps rendering safe when a formula is incomplete or invalid during streaming', () => {
    const { container, rerender } = render(<MarkdownRenderer content={String.raw`Text \[\frac{a}{`} />);
    expect(container.textContent).toContain('Text');
    rerender(<MarkdownRenderer content={String.raw`$$\frac{a}$$`} />);
    expect(container.querySelector('.katex-error')).toBeTruthy();
    rerender(<MarkdownRenderer content={String.raw`\[\frac{a}{b}\]`} />);
    expect(container.querySelector('.katex')).toBeTruthy();
    expect(container.querySelector('.katex-error')).toBeNull();
  });

  it('does not allow formula commands to create trusted links or raw HTML', () => {
    const { container } = render(<MarkdownRenderer content={String.raw`$\href{javascript:alert(1)}{click}$

$\htmlClass{injected}{x}$

<script>window.compromised = true</script>`} />);
    expect(container.querySelector('a[href^="javascript:"]')).toBeNull();
    expect(container.querySelector('.injected')).toBeNull();
    expect(container.querySelector('script')).toBeNull();
  });
});
