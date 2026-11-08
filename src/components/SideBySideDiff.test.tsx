import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { SideBySideDiff } from './SideBySideDiff';

// Render each parsed row; geometry/scrolling requires a real browser separately.
vi.mock('@tanstack/react-virtual', () => ({
  useVirtualizer: ({ count }: { count: number }) => ({
    getTotalSize: () => count * 20,
    getVirtualItems: () => Array.from({ length: count }, (_, index) => ({ index, key: index, start: index * 20 })),
    measureElement: vi.fn(),
  }),
}));

describe('SideBySideDiff patch parsing', () => {
  it('preserves source lines starting with double minus or plus inside a hunk', () => {
    const { container } = render(<SideBySideDiff diffText={'--- a/options.txt\n+++ b/options.txt\n@@ -4 +4 @@\n---flag\n+++counter\n'} />);
    const oldLine = screen.getByText((_, node) => node?.textContent === '--flag' && !Array.from(node.children).some((child) => child.textContent === '--flag'));
    expect(oldLine).toBeTruthy();
    expect(screen.getByText((_, node) => node?.textContent === '++counter' && !Array.from(node.children).some((child) => child.textContent === '++counter'))).toBeTruthy();
    const row = oldLine.closest('[data-index]');
    expect(row?.textContent).toContain('4');
    expect(container.querySelectorAll('[data-index]')).toHaveLength(2);
  });

  it('ignores metadata and no-newline markers without adding phantom line numbers', () => {
    const patch = 'diff --git a/file b/file\nindex 123..456 100644\n--- a/file\n+++ b/file\n@@ -8,2 +8,2 @@\n first\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n';
    const { container } = render(<SideBySideDiff diffText={patch} />);
    expect(screen.queryByText(/diff --git/)).toBeNull();
    expect(screen.queryByText(/index 123/)).toBeNull();
    expect(screen.queryByText(/No newline at end/)).toBeNull();
    expect(container.querySelectorAll('[data-index]')).toHaveLength(3);
    expect(screen.getByText('old').closest('[data-index]')?.textContent).toBe('9old9new');
  });

  it('restarts counters for each file and accepts empty-range hunks', () => {
    const patch = '--- /dev/null\n+++ b/new\n@@ -0,0 +1 @@\n+created\ndiff --git a/other b/other\n--- a/other\n+++ b/other\n@@ -20 +30 @@\n-before\n+after\n';
    render(<SideBySideDiff diffText={patch} />);
    expect(screen.getByText('created').closest('[data-index]')?.textContent).toBe('1created');
    expect(screen.getByText('before').closest('[data-index]')?.textContent).toBe('20before30after');
  });
});
