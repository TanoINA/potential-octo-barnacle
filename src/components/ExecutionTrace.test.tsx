import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { ToolCallItem } from '../types/agent';
import { ExecutionTrace } from './ExecutionTrace';

function tool(overrides: Partial<ToolCallItem> = {}): ToolCallItem {
  return { id: 'tool-1', name: 'write_file', args: { path: 'report.txt' }, status: 'error', result: 'Permission denied', ...overrides };
}

describe('ExecutionTrace truthful details', () => {
  it.each([
    ['write_file', { path: 'report.txt' }, 'Permission denied'],
    ['use_skill', { name: 'missing-skill' }, 'Skill not found'],
    ['mcp__example__lookup', { query: '<script>unsafe()</script>' }, '<img src=x onerror=unsafe()> Remote request failed'],
  ])('exposes failed %s arguments and inert results in a native disclosure', (name, args, result) => {
    const { container } = render(<ExecutionTrace toolCalls={[tool({ name, args, result })]} />);
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
    const summary = screen.getByRole('button', { name: '1 step cancelled' });
    fireEvent.click(summary);
    const row = screen.getByRole('button', { name: `${name} Cancelled` });
    expect(screen.queryByText(/Loaded|Wrote|Ran |Failed/)).toBeNull();
    fireEvent.click(row);
    expect(screen.getByText('Stopped by user')).toBeTruthy();
  });

  it('keeps artifact selection separate from its accessible details disclosure', () => {
    const onSelectArtifact = vi.fn();
    render(<ExecutionTrace toolCalls={[tool({ artifactId: 'artifact-1' })]} onSelectArtifact={onSelectArtifact} />);
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
    fireEvent.click(screen.getByRole('button', { name: 'ran 1 command' }));
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
    fireEvent.click(screen.getByRole('button', { name: '1 step executed' }));
    fireEvent.click(screen.getByRole('button', { name: 'mcp__example__lookup' }));
    expect(screen.getByText(raw)).toBeTruthy();
    expect(screen.getByText('Found a record')).toBeTruthy();
  });

  it('marks nonzero exits as failed even when tool status is done', () => {
    render(<ExecutionTrace toolCalls={[tool({ status: 'done', exitCode: 1 })]} />);
    expect(screen.getByRole('button', { name: 'write_file Failed' }).getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('Permission denied')).toBeTruthy();
    expect(screen.queryByText(/Wrote/)).toBeNull();
  });
});
