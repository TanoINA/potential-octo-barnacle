import { render, fireEvent } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { MarkdownRenderer } from './MarkdownRenderer';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

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
});
