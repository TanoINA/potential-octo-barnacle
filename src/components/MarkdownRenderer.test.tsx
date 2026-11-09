import { render, fireEvent } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { MarkdownRenderer } from './MarkdownRenderer';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

describe('MarkdownRenderer', () => {
  it.each([
    ['lone high surrogate', 'Write-Output \uD800', 'Write-Output \uFFFD'],
    ['lone low surrogate', 'Write-Output \uDC00', 'Write-Output \uFFFD'],
    ['mixed surrogates', 'Write-Output \uD840\uDC00\uD800\uD840\uDC00\uDC00', 'Write-Output \uD840\uDC00\uFFFD\uD840\uDC00\uFFFD'],
    ['valid surrogate pair', 'Write-Output \uD840\uDC00', 'Write-Output \uD840\uDC00'],
  ])('renders fenced code containing %s with safe context-menu metadata', (_, code, expected) => {
    const { container } = render(<MarkdownRenderer content={'```powershell\n' + code + '\n```'} />);
    const block = container.querySelector('[data-code-block="true"]');
    expect(block).not.toBeNull();
    expect(decodeURIComponent(block!.getAttribute('data-code-content')!)).toBe(expected);
    expect(block!.getAttribute('data-code-lang')).toBe('powershell');
    expect(container.querySelector('pre code')?.textContent).toBe(code);
  });

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

  it('preserves Windows drive file links and file schemes without sanitizing them to empty', () => {
    const markdown = [
      '[win](D:/UNIKOM/Sistem%20Operasi/file.docx)',
      '[fileUrl](file:///D:/UNIKOM/file.docx)',
      '[web](https://google.com)',
      '[bad](javascript:alert(1))',
    ].join('\n\n');

    const { container } = render(<MarkdownRenderer content={markdown} />);
    const links = Array.from(container.querySelectorAll('a'));

    const winLink = links.find((a) => a.textContent?.includes('win'));
    const fileLink = links.find((a) => a.textContent?.includes('fileUrl'));
    const webLink = links.find((a) => a.textContent?.includes('web'));
    const badLink = links.find((a) => a.textContent?.includes('bad'));

    expect(winLink?.getAttribute('href')).toBe('D:/UNIKOM/Sistem%20Operasi/file.docx');
    expect(fileLink?.getAttribute('href')).toBe('file:///D:/UNIKOM/file.docx');
    expect(webLink?.getAttribute('href')).toBe('https://google.com');
    expect(badLink?.getAttribute('href')).toBe('');
  });

  it('invokes open_path_or_url with decoded path when file link is clicked in Tauri environment', () => {
    (window as any).__TAURI_INTERNALS__ = {};
    const { container } = render(
      <MarkdownRenderer content="[Report](file:///D:/UNIKOM/Sistem%20Operasi/Tugas%201/doc.docx)" />
    );
    const link = container.querySelector('a');
    expect(link).not.toBeNull();
    fireEvent.click(link!);

    expect(invoke).toHaveBeenCalledWith('open_path_or_url', {
      pathOrUrl: 'file:///D:/UNIKOM/Sistem Operasi/Tugas 1/doc.docx',
    });
    delete (window as any).__TAURI_INTERNALS__;
  });

  it('reconciles a streaming plan through the real Markdown pipeline without remounting it', () => {
    const fence = '\x60\x60\x60';
    const { getByText, getByTestId, queryByText, rerender } = render(
      <MarkdownRenderer content={fence + 'plan\n- [ ] Inspect files\n- [ ] Implement rec'} />
    );
    const card = getByTestId('interactive-plan-card');
    fireEvent.click(getByText('Inspect files'));

    rerender(<MarkdownRenderer content={fence + 'plan\n- [ ] Inspect files\n- [ ] Implement reconciliation\n- [x] Run tests\n' + fence} />);
    expect(getByTestId('interactive-plan-card')).toBe(card);
    expect(queryByText('Implement rec')).toBeNull();
    expect(getByText('Implement reconciliation')).toBeTruthy();
    expect(getByText('Run tests').className).toContain('line-through');
    expect(getByText('2/3 completed')).toBeTruthy();

    rerender(<MarkdownRenderer content={fence + 'plan\n- [ ] Inspect files\n- [ ] Implement reconciliation\n- [x] Run tests\n' + fence + '\n\nUnrelated prose update'} />);
    expect(getByTestId('interactive-plan-card')).toBe(card);
    expect(getByText('Inspect files').className).toContain('line-through');
    expect(getByText('2/3 completed')).toBeTruthy();
  });

  it('preserves manual plan toggles when only surrounding Markdown changes', () => {
    const fence = '\x60\x60\x60';
    const content = fence + 'plan\n- [ ] Keep my toggle\n' + fence;
    const { getByText, getByTestId, rerender } = render(<MarkdownRenderer content={content} />);
    const card = getByTestId('interactive-plan-card');
    fireEvent.click(getByText('Keep my toggle'));
    rerender(<MarkdownRenderer content={content + '\n\nMore prose'} />);
    expect(getByTestId('interactive-plan-card')).toBe(card);
    expect(getByText('1/1 completed')).toBeTruthy();
  });

  it('switches between plan and normal code fences without changing hook order', () => {
    const fence = '\x60\x60\x60';
    const { container, queryByTestId, getByText, rerender } = render(
      <MarkdownRenderer content={fence + 'plan\n- [ ] Build\n' + fence} />
    );
    rerender(<MarkdownRenderer content={fence + 'text\nnormal code\n' + fence} />);
    expect(queryByTestId('interactive-plan-card')).toBeNull();
    expect(container.querySelector('pre code')?.textContent).toBe('normal code');
    rerender(<MarkdownRenderer content={fence + 'plan\n- [x] Build\n' + fence} />);
    expect(getByText('1/1 completed')).toBeTruthy();
  });

});
