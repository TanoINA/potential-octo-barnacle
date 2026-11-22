import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ToolCallItem } from '../types/agent';
import { ExecutionTrace, parseSearchLinks } from './ExecutionTrace';

function tool(overrides: Partial<ToolCallItem> = {}): ToolCallItem {
  return { id: 'tool-1', name: 'write_file', args: { path: 'report.txt' }, status: 'error', result: 'Permission denied', ...overrides };
}

describe('ExecutionTrace grouped activity', () => {
  it('separates main actions from collapsed subagent work and preserves child details', () => {
    const main = Array.from({ length: 84 }, (_, index) => tool({ id: `turn_${index}`, name: 'read_file', args: { path: `main-${index}.txt` }, status: 'done' }));
    const children = Array.from({ length: 586 }, (_, index) => tool({
      id: `sub-${index % 14}/turn_${index}`, name: 'read_file', args: { path: `child-${index}.txt` },
      status: index === 0 ? 'error' : 'done', result: index === 0 ? 'Child permission denied' : 'Read result',
    }));
    render(<ExecutionTrace toolCalls={[...main, ...children]} />);
    const header = screen.getByRole('button', { name: '84 main actions · 14 subagents 1 failed' });
    expect(screen.getByText('84 main actions · 14 subagents').classList.contains('text-[#f85149]')).toBe(false);
    expect(screen.getByText('1 failed').classList.contains('text-[#f85149]')).toBe(true);
    fireEvent.click(header);
    expect(screen.getByRole('button', { name: 'Read main-0.txt' })).toBeTruthy();
    expect(screen.queryByText('Child permission denied')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Read child-14.txt' })).toBeNull();
    const group = screen.getByRole('button', { name: 'Subagent sub-0 · 42 actions 1 failed' });
    expect(group.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(group);
    expect(group.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('Child permission denied')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Read child-14.txt' })).toBeTruthy();
    fireEvent.click(group);
    expect(screen.queryByText('Child permission denied')).toBeNull();
  });

  it('keeps live child work collapsed across updates and preserves artifact navigation when opened', () => {
    const select = vi.fn();
    const child = tool({ id: 'sub-live/tool', status: 'running', result: '', artifactId: 'artifact' });
    const { rerender } = render(<ExecutionTrace toolCalls={[child]} onSelectArtifact={select} />);
    const group = screen.getByRole('button', { name: 'Subagent sub-live · 1 action in progress' });
    expect(group.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('button', { name: 'Writing report.txt' })).toBeNull();
    rerender(<ExecutionTrace toolCalls={[{ ...child, status: 'error', result: 'Denied' }]} isStreaming onSelectArtifact={select} />);
    expect(group.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(group);
    fireEvent.click(screen.getByRole('button', { name: 'write_file Failed' }));
    expect(select).toHaveBeenCalledExactlyOnceWith('artifact');
    expect(screen.getByText('Denied')).toBeTruthy();
    expect(screen.getAllByText('1 failed')).toHaveLength(2);
  });

  it('counts unscoped and legacy IDs as main actions, not guessed subagents', () => {
    render(<ExecutionTrace toolCalls={[
      tool({ id: 'legacy/tool', status: 'done' }),
      tool({ id: 'sub-without-scope', status: 'cancelled', exitCode: 130 }),
    ]} />);
    expect(screen.getByRole('button', { name: '2 main actions cancelled' })).toBeTruthy();
    expect(screen.queryByText(/failed/i)).toBeNull();
  });
});

describe('ExecutionTrace command encoding safety', () => {
  it.each([
    ['lone high surrogate', 'Write-Output \uD800', 'Write-Output \uFFFD'],
    ['lone low surrogate', 'Write-Output \uDC00', 'Write-Output \uFFFD'],
    ['mixed surrogates', 'Write-Output \uD83D\uDE80\uD800\uD83D\uDE80\uDC00', 'Write-Output \uD83D\uDE80\uFFFD\uD83D\uDE80\uFFFD'],
    ['valid surrogate pair', 'Write-Output \uD83D\uDE80', 'Write-Output \uD83D\uDE80'],
  ])('renders a raw partial JSON command containing %s', (_, command, expected) => {
    const raw = '{"command":"' + command;
    const { container } = render(<ExecutionTrace toolCalls={[tool({ name: 'run_command', args: {}, argumentsRaw: raw, status: 'running', result: '' })]} />);
    const row = container.querySelector('[data-code-block="true"]');
    expect(row).not.toBeNull();
    expect(decodeURIComponent(row!.getAttribute('data-code-content')!)).toBe(expected);
    expect(row!.getAttribute('data-code-lang')).toBe('powershell');
    expect(row!.textContent).toContain(command);
    expect(container.querySelector('pre[data-selectable-text="true"]')?.textContent).toBe(raw);
  });
});

describe('ExecutionTrace render stability', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('does not reparse static arguments on elapsed ticks or unchanged tool identities', () => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);
    const staticRaw = JSON.stringify({ path: 'trace-render-static.txt', marker: 'trace-render-static' });
    const activeRaw = JSON.stringify({ path: 'trace-render-active.txt', marker: 'trace-render-active' });
    const staticTool = tool({ id: 'static', name: 'read_file', args: {}, argumentsRaw: staticRaw, status: 'done', result: 'Static result' });
    const activeTool = tool({ id: 'active', name: 'read_file', args: {}, argumentsRaw: activeRaw, status: 'running', startedAt: 7_000, result: '' });
    const realParse = JSON.parse;
    const parse = vi.spyOn(JSON, 'parse').mockImplementation((...args: Parameters<typeof JSON.parse>) => realParse(...args));
    const countParses = (raw: string) => parse.mock.calls.filter(([input]) => input === raw).length;
    const { rerender } = render(<ExecutionTrace toolCalls={[staticTool, activeTool]} />);
    expect(countParses(staticRaw)).toBeGreaterThan(0);
    expect(countParses(activeRaw)).toBeGreaterThan(0);
    expect(screen.getByRole('button', { name: /Reading trace-render-active.txt.*3s/ })).toBeTruthy();
    parse.mockClear();

    for (let elapsed = 4; elapsed <= 6; elapsed += 1) {
      act(() => { vi.advanceTimersByTime(1000); });
      expect(screen.getByRole('button', { name: new RegExp(`Reading trace-render-active.txt.*${elapsed}s`) })).toBeTruthy();
      expect(countParses(staticRaw)).toBe(0);
      expect(countParses(activeRaw)).toBe(0);
    }

    rerender(<ExecutionTrace toolCalls={[staticTool, activeTool]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Read trace-render-static.txt' }));
    expect(screen.getByText('Static result')).toBeTruthy();
    expect(countParses(staticRaw)).toBe(0);
    expect(countParses(activeRaw)).toBe(0);

    const updatedRaw = JSON.stringify({ path: 'trace-render-updated.txt', marker: 'trace-render-updated' });
    rerender(<ExecutionTrace toolCalls={[staticTool, { ...activeTool, argumentsRaw: updatedRaw }]} />);
    expect(screen.getByRole('button', { name: /Reading trace-render-updated.txt.*6s/ })).toBeTruthy();
    expect(countParses(updatedRaw)).toBeGreaterThan(0);
    expect(countParses(staticRaw)).toBe(0);
    expect(countParses(activeRaw)).toBe(0);
    parse.mockClear();
    act(() => { vi.advanceTimersByTime(1000); });
    expect(screen.getByRole('button', { name: /Reading trace-render-updated.txt.*7s/ })).toBeTruthy();
    expect(countParses(updatedRaw)).toBe(0);
    expect(countParses(staticRaw)).toBe(0);

    const header = screen.getByRole('button', { name: /2 main actions in progress/ });
    fireEvent.click(header);
    fireEvent.click(header);
    expect(screen.getByText('Static result')).toBeTruthy();
    expect(countParses(staticRaw)).toBe(0);
    expect(countParses(updatedRaw)).toBe(0);
  });

  it('ticks only visible active durations once per second without remapping the trace', () => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);
    const tools = [
      tool({ id: 'static', name: 'read_file', args: { path: 'static.txt' }, status: 'done' }),
      tool({ id: 'active', name: 'read_file', args: { path: 'active.txt' }, status: 'running', startedAt: 7_000 }),
      tool({ id: 'pending', name: 'read_file', args: { path: 'pending.txt' }, status: 'pending', startedAt: 9_000 }),
    ];
    const map = vi.spyOn(tools, 'map');
    const interval = vi.spyOn(globalThis, 'setInterval');
    const { rerender, unmount } = render(<ExecutionTrace toolCalls={tools} isStreaming />);
    expect(interval.mock.calls.map(([, delay]) => delay)).toEqual([1000, 1000]);
    expect(vi.getTimerCount()).toBe(2);
    map.mockClear();

    act(() => { vi.advanceTimersByTime(999); });
    expect(screen.getByRole('button', { name: /Reading active.txt.*3s/ })).toBeTruthy();
    expect(map).not.toHaveBeenCalled();
    act(() => { vi.advanceTimersByTime(1); });
    expect(screen.getByRole('button', { name: /Reading active.txt.*4s/ })).toBeTruthy();
    expect(screen.getByRole('button', { name: /Reading pending.txt.*2s/ })).toBeTruthy();
    expect(map).not.toHaveBeenCalled();

    const header = screen.getByRole('button', { name: /3 main actions in progress/ });
    fireEvent.click(header);
    expect(header.getAttribute('aria-expanded')).toBe('false');
    expect(vi.getTimerCount()).toBe(0);
    act(() => { vi.advanceTimersByTime(2000); });
    fireEvent.click(header);
    expect(vi.getTimerCount()).toBe(2);
    expect(screen.getByRole('button', { name: /Reading active.txt.*6s/ })).toBeTruthy();
    expect(screen.getByRole('button', { name: /Reading pending.txt.*4s/ })).toBeTruthy();

    rerender(<ExecutionTrace toolCalls={tools.map((item) => ({ ...item, status: 'done' as const, durationMs: 500 }))} isStreaming />);
    expect(header.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getAllByText('500ms')).toHaveLength(3);
    expect(vi.getTimerCount()).toBe(0);
    rerender(<ExecutionTrace toolCalls={tools} isStreaming />);
    expect(vi.getTimerCount()).toBe(2);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('ExecutionTrace truthful details', () => {
  it.each([
    { status: 'done' as const, exitCode: 0 },
    { status: 'error' as const },
    { status: 'done' as const, exitCode: 1 },
    { status: 'cancelled' as const },
  ])('auto-collapses after running ends ($status, exit $exitCode) and allows reopening', (completed) => {
    const { rerender } = render(<ExecutionTrace toolCalls={[tool({ status: 'running' })]} isStreaming />);
    const header = screen.getByRole('button', { name: /1 main action in progress/ });
    expect(header.getAttribute('aria-expanded')).toBe('true');
    // An explicit open choice during the live phase must not defeat auto-collapse.
    fireEvent.click(header);
    fireEvent.click(header);
    rerender(<ExecutionTrace toolCalls={[tool(completed)]} />);
    expect(header.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText('Permission denied')).toBeNull();
    fireEvent.click(header);
    expect(header.getAttribute('aria-expanded')).toBe('true');
    rerender(<ExecutionTrace toolCalls={[tool({ ...completed, durationMs: 500 })]} />);
    expect(header.getAttribute('aria-expanded')).toBe('true');
  });

  it.each([
    { status: 'error' as const },
    { status: 'done' as const, exitCode: 2 },
  ])('keeps a separate red failure count visible while collapsed ($status)', (failure) => {
    render(<ExecutionTrace toolCalls={[tool(failure)]} />);
    expect(screen.getByText('1 main action').classList.contains('text-[#f85149]')).toBe(false);
    expect(screen.getByText('1 failed').classList.contains('text-[#f85149]')).toBe(true);
    expect(screen.getByRole('button', { name: '1 main action 1 failed' }).getAttribute('aria-expanded')).toBe('false');
  });

  it('bounds a long expanded trace with an internal contained scrollbar', () => {
    const tools = Array.from({ length: 177 }, (_, index) => tool({ id: `tool-${index}`, status: 'done' }));
    const { container } = render(<ExecutionTrace toolCalls={tools} />);
    fireEvent.click(screen.getByRole('button', { name: '177 main actions' }));
    const list = container.querySelector('.max-h-80');
    expect(list).toBeTruthy();
    expect(list?.classList.contains('overflow-y-auto')).toBe(true);
    expect(list?.classList.contains('overscroll-contain')).toBe(true);
    expect(list?.children.length).toBe(177);
  });

  it('opens a new live phase and keeps it open until streaming itself ends', () => {
    const doneTool = tool({ status: 'done', exitCode: 0 });
    const { rerender } = render(<ExecutionTrace toolCalls={[doneTool]} />);
    const header = screen.getByRole('button', { name: '1 main action' });
    expect(header.getAttribute('aria-expanded')).toBe('false');
    rerender(<ExecutionTrace toolCalls={[tool({ status: 'pending' })]} />);
    expect(header.getAttribute('aria-expanded')).toBe('true');
    // Live errors must not collapse a trace while the message is still streaming.
    rerender(<ExecutionTrace toolCalls={[tool()]} isStreaming />);
    expect(header.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('1 main action').classList.contains('text-[#f85149]')).toBe(false);
    expect(screen.getByText('1 failed').classList.contains('text-[#f85149]')).toBe(true);
    rerender(<ExecutionTrace toolCalls={[tool()]} />);
    expect(header.getAttribute('aria-expanded')).toBe('false');
  });

  it('does not accent cancelled or successful trace summaries as failures', () => {
    const { rerender } = render(<ExecutionTrace toolCalls={[tool({ status: 'cancelled', exitCode: 130 })]} />);
    expect(screen.getByText('1 main action').classList.contains('text-[#f85149]')).toBe(false);
    rerender(<ExecutionTrace toolCalls={[tool({ status: 'done', exitCode: 0 })]} />);
    expect(screen.getByText('1 main action').classList.contains('text-[#f85149]')).toBe(false);
  });

  it('keeps restored failures collapsed by default', () => {
    render(<ExecutionTrace toolCalls={[tool()]} />);
    expect(screen.getByRole('button', { name: '1 main action 1 failed' }).getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText('Permission denied')).toBeNull();
  });
  it.each([
    ['write_file', { path: 'report.txt' }, 'Permission denied'],
    ['use_skill', { name: 'missing-skill' }, 'Skill not found'],
    ['mcp__example__lookup', { query: '<script>unsafe()</script>' }, '<img src=x onerror=unsafe()> Remote request failed'],
  ])('exposes failed %s arguments and inert results in a native disclosure', (name, args, result) => {
    const { container } = render(<ExecutionTrace toolCalls={[tool({ name, args, result })]} />);
    fireEvent.click(screen.getByRole('button', { name: '1 main action 1 failed' }));
    const row = screen.getByRole('button', { name: new RegExp(`${name}.*Failed`) });
    expect(row.tagName).toBe('BUTTON');
    expect(row.getAttribute('type')).toBe('button');
    expect(row.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText(result)).toBeTruthy();
    expect(screen.getByText((_, element) => element?.tagName === 'PRE' && element.textContent === JSON.stringify(args, null, 2))).toBeTruthy();
    expect(container.querySelector('script, img')).toBeNull();
    expect(screen.queryByText(/Loaded skill|Wrote/)).toBeNull();
    const detailsId = row.getAttribute('aria-controls');
    expect(detailsId && document.getElementById(detailsId)).toBeTruthy();
    fireEvent.click(row);
    expect(row.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText(result)).toBeNull();
    row.focus();
    expect(document.activeElement).toBe(row);
    // Native button activation delivers a click for both Enter and Space.
    fireEvent.click(row);
    expect(screen.getByText(result)).toBeTruthy();
  });

  it.each(['write_file', 'use_skill', 'run_command'])('does not describe cancelled %s work as successful', (name) => {
    render(<ExecutionTrace toolCalls={[tool({ name, status: 'cancelled', exitCode: 130, result: 'Stopped by user' })]} />);
    const summary = screen.getByRole('button', { name: '1 main action cancelled' });
    fireEvent.click(summary);
    const row = screen.getByRole('button', { name: `${name} Cancelled` });
    expect(screen.queryByText(/Loaded|Wrote|Ran |Failed/)).toBeNull();
    fireEvent.click(row);
    expect(screen.getByText('Stopped by user')).toBeTruthy();
  });

  it('keeps artifact selection separate from its accessible details disclosure', () => {
    const onSelectArtifact = vi.fn();
    render(<ExecutionTrace toolCalls={[tool({ artifactId: 'artifact-1' })]} onSelectArtifact={onSelectArtifact} />);
    fireEvent.click(screen.getByRole('button', { name: '1 main action 1 failed' }));
    const row = screen.getByRole('button', { name: 'write_file Failed' });
    fireEvent.click(row);
    expect(onSelectArtifact).toHaveBeenCalledExactlyOnceWith('artifact-1');
    const disclosure = screen.getByRole('button', { name: 'Details for write_file' });
    fireEvent.click(disclosure);
    expect(screen.queryByText('Permission denied')).toBeNull();
    expect(onSelectArtifact).toHaveBeenCalledTimes(1);
    expect(row.contains(disclosure)).toBe(false);
  });

  it('keeps successful run_command terminal output and exit status', () => {
    render(<ExecutionTrace toolCalls={[tool({ name: 'run_command', args: { command: 'echo hello' }, status: 'done', result: 'hello', exitCode: 0 })]} />);
    fireEvent.click(screen.getByRole('button', { name: '1 main action' }));
    const row = screen.getByRole('button', { name: 'Ran "echo hello"' });
    expect(row.getAttribute('aria-expanded')).toBe('false');
    fireEvent.click(row);
    expect(screen.getByText('hello')).toBeTruthy();
    expect(screen.getByText('exit 0')).toBeTruthy();
    expect(screen.getByRole('button', { name: 'Copy' })).toBeTruthy();
    expect(screen.queryByText(/Failed|Cancelled/)).toBeNull();
  });

  it('exposes successful nonterminal results and incomplete raw arguments', () => {
    const raw = '{"query":"partial';
    render(<ExecutionTrace toolCalls={[tool({ name: 'mcp__example__lookup', args: {}, argumentsRaw: raw, status: 'done', result: 'Found a record' })]} />);
    fireEvent.click(screen.getByRole('button', { name: '1 main action' }));
    fireEvent.click(screen.getByRole('button', { name: 'mcp__example__lookup' }));
    expect(screen.getByText(raw)).toBeTruthy();
    expect(screen.getByText('Found a record')).toBeTruthy();
  });

  it('marks nonzero exits as failed even when tool status is done', () => {
    render(<ExecutionTrace toolCalls={[tool({ status: 'done', exitCode: 1 })]} />);
    fireEvent.click(screen.getByRole('button', { name: '1 main action 1 failed' }));
    expect(screen.getByRole('button', { name: 'write_file Failed' }).getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('Permission denied')).toBeTruthy();
    expect(screen.queryByText(/Wrote/)).toBeNull();
  });

  it('does not treat null exitCode from native nonterminal tools as failed', () => {
    render(<ExecutionTrace toolCalls={[tool({ name: 'read_file', args: { filePath: 'README.md' }, status: 'done', exitCode: null as unknown as number, result: 'File content' })]} />);
    expect(screen.queryByText(/Failed/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: '1 main action' }));
    expect(screen.getByRole('button', { name: 'Read README.md' })).toBeTruthy();
  });

  it('formats search_web with dedicated label and renders clickable source chips', () => {
    const searchResult = `Search results for "rust async":

1. [Asynchronous Programming in Rust](https://rust-lang.github.io/async-book/)
   An introduction to async in Rust.

2. [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
   Get started with Tokio.`;

    render(
      <ExecutionTrace
        toolCalls={[
          tool({
            name: 'search_web',
            args: { query: 'rust async' },
            status: 'done',
            result: searchResult,
          }),
        ]}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: '1 main action' }));
    const row = screen.getByRole('button', { name: 'Searched web for "rust async"' });
    expect(row).toBeTruthy();

    fireEvent.click(row);
    expect(screen.getByText('Discovered Web Sources (2)')).toBeTruthy();
    expect(screen.getByText('Asynchronous Programming in Rust')).toBeTruthy();
    expect(screen.getByText('Tokio Tutorial')).toBeTruthy();

    const link = screen.getByTitle(/Asynchronous Programming in Rust/);
    expect(link.getAttribute('href')).toBe('https://rust-lang.github.io/async-book/');
  });

  it('drops non-http links from search results', () => {
    const raw = [
      '1. [Evil](javascript:alert(1))',
      '2. [Safe](https://example.com/docs)',
    ].join('\n');
    expect(parseSearchLinks(raw)).toEqual([
      { title: 'Safe', url: 'https://example.com/docs', snippet: undefined },
    ]);
  });
});
