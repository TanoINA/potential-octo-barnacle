import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { ChatCanvas } from './ChatCanvas';

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./MessageRow', () => ({ MessageRow: () => null }));

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
});

function setup(overrides: Partial<ComponentProps<typeof ChatCanvas>> = {}) {
  const onSendMessage = vi.fn(async () => true);
  const view = render(<ChatCanvas
    messages={[]} isStreaming={false} stopRequested={false}
    onSendMessage={onSendMessage} onStop={vi.fn()} onSelectArtifact={vi.fn()}
    onClearSession={vi.fn()} onNewSession={() => 'new'} onOpenSettings={vi.fn()}
    onOpenModelPicker={vi.fn()} effectiveConfig={null} sessionTitle="UI test"
    artifactCount={0} isArtifactOpen={false} onToggleArtifacts={vi.fn()}
    onListFiles={async () => []} {...overrides}
  />);
  return { ...view, onSendMessage };
}

describe('ChatCanvas calm presentation', () => {
  it('offers a compact task prompt and plain examples instead of a branded feature landing', () => {
    const { container } = setup();
    const heading = screen.getByRole('heading', { name: 'What would you like to work on?' });
    const emptyState = heading.parentElement!;
    expect(emptyState.classList.contains('ui-enter')).toBe(true);
    expect(within(emptyState).getByText('Find the code responsible for a failing test.')).toBeTruthy();
    expect(within(emptyState).getByText('Explain how this workspace is organized.')).toBeTruthy();
    expect(within(emptyState).getByText(/@ for workspace files/)).toBeTruthy();
    expect(within(emptyState).getByText(/\/ for commands/)).toBeTruthy();
    expect(emptyState.querySelector('svg')).toBeNull();
    expect(emptyState.querySelector('button')).toBeNull();
    expect(screen.queryByText('Native workspace tools')).toBeNull();
    expect(screen.queryByText('Provider flexibility')).toBeNull();
    expect(container.querySelector('[class*="gradient"], [class*="animate-bounce"], [class*="animate-pulse"], [class*="animate-ping"]')).toBeNull();
    const composer = screen.getByRole('textbox').parentElement!.parentElement!;
    expect(composer.classList.contains('rounded-xl')).toBe(true);
  });

  it('uses shared menu entry classes and a static drop affordance', () => {
    const { container } = setup();
    fireEvent.click(screen.getByLabelText('Execution mode'));
    expect(screen.getByRole('menu', { name: 'Execution mode selection' }).classList.contains('ui-menu')).toBe(true);
    fireEvent.click(screen.getByLabelText('Add Context'));
    expect(screen.getByRole('button', { name: 'Files' }).parentElement!.classList.contains('ui-menu')).toBe(true);
    fireEvent.dragOver(container.firstElementChild!);
    const dropOverlay = screen.getByText('Drop files or folders here').parentElement!;
    expect(dropOverlay.classList.contains('ui-enter')).toBe(true);
    expect(dropOverlay.querySelector('[class*="animate-bounce"], [class*="animate-pulse"], [class*="animate-ping"]')).toBeNull();
  });

  it('preserves composer focus events, prompt insertion, and Enter submission from the empty state', async () => {
    const { onSendMessage } = setup();
    const input = screen.getByLabelText('Message AetherAgent') as HTMLTextAreaElement;
    fireEvent(window, new Event('aether:focus-composer'));
    expect(document.activeElement).toBe(input);
    fireEvent(window, new CustomEvent('aether:insert-prompt', { detail: { text: 'Inspect the failing test' } }));
    expect(input.value).toBe('Inspect the failing test');
    fireEvent.keyDown(input, { key: 'Enter', shiftKey: true });
    expect(onSendMessage).not.toHaveBeenCalled();
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(onSendMessage).toHaveBeenCalledWith('Inspect the failing test', undefined, expect.any(Object)));
  });

  it('renders context meter accurately without double-counting segmented messages and detects model limits', () => {
    const text = 'Hello world, this is a test response.';
    const messages = [
      {
        id: 'msg-1',
        role: 'user' as const,
        content: 'Hi there',
        timestamp: 1000,
      },
      {
        id: 'msg-2',
        role: 'assistant' as const,
        content: text,
        timestamp: 2000,
        segments: [{ id: 'seg-1', text }],
      },
    ];
    setup({
      messages,
      effectiveConfig: {
        provider: {
          id: 'p-1',
          name: 'Anthropic',
          kind: 'anthropic',
          baseUrl: 'https://api.anthropic.com',
          apiKey: 'key',
          model: 'claude-3-5-sonnet-20241022',
        },
        thinking: false,
        reasoningEffort: 'medium',
        usesSettingsDefault: true,
      },
    });

    const meter = screen.getByTestId('context-meter');
    expect(meter).toBeTruthy();
    // Denominator should be 200k for Claude 3.5 Sonnet
    expect(meter.textContent).toContain('/ 200k tokens');
  });
});
