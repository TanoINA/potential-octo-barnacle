import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ComponentProps } from 'react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { ChatCanvas, getModelContextWindow } from './ChatCanvas';

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

  it('accurately resolves dynamic context window sizes and custom overrides', () => {
    // Frontier era 1M generations (Claude 5, GPT-6, DeepSeek v4, Kimi k3, GLM-5, Qwen 3, etc.)
    expect(getModelContextWindow('claude-sonnet-5.5')).toBe(1_000_000);
    expect(getModelContextWindow('claude-haiku-5.5')).toBe(1_000_000);
    expect(getModelContextWindow('claude-fable-5.1')).toBe(1_000_000);
    expect(getModelContextWindow('gpt-6-astra')).toBe(1_000_000);
    expect(getModelContextWindow('gpt-5.6-sol')).toBe(1_000_000);
    expect(getModelContextWindow('deepseek-v4-flash')).toBe(1_000_000);
    expect(getModelContextWindow('deepseek-v4.1-flash')).toBe(1_000_000);
    expect(getModelContextWindow('kimi-k3.1-pro')).toBe(1_000_000);
    expect(getModelContextWindow('glm-5.3-flash')).toBe(1_000_000);
    expect(getModelContextWindow('qwen-3.8-max')).toBe(1_000_000);
    expect(getModelContextWindow('mimo-v2.6-flash')).toBe(1_000_000);
    expect(getModelContextWindow('hy4-preview')).toBe(1_000_000);
    expect(getModelContextWindow('minimax-m3')).toBe(1_000_000);

    // 200k - 256k generations
    expect(getModelContextWindow('claude-3-5-sonnet')).toBe(200_000);
    expect(getModelContextWindow('minimax-m2.7')).toBe(200_000);
    expect(getModelContextWindow('kimi-k2.6')).toBe(256_000);
    expect(getModelContextWindow('hy3-preview')).toBe(256_000);
    expect(getModelContextWindow('nemotron-3.5-lightning')).toBe(256_000);

    // Universal explicit token size tags
    expect(getModelContextWindow('custom-proxy/meta-llama-256k')).toBe(256_000);
    expect(getModelContextWindow('my-provider/model-1m')).toBe(1_000_000);
    expect(getModelContextWindow('moonshot-v1-32k')).toBe(32_000);
    expect(getModelContextWindow('gpt-3.5-turbo-4k')).toBe(4_000);
    expect(getModelContextWindow('any-engine-1024k')).toBe(1_024_000);
    expect(getModelContextWindow('big-model-2m')).toBe(2_000_000);

    // Parameter count in 'm' (>10m) should NOT be treated as context tokens
    expect(getModelContextWindow('facebook/opt-350m')).toBe(128_000);
    expect(getModelContextWindow('smollm-135m')).toBe(128_000);

    // Baseline 128k modern standard
    expect(getModelContextWindow('gpt-4o')).toBe(128_000);
    expect(getModelContextWindow('deepseek-chat')).toBe(128_000);
    expect(getModelContextWindow('deepseek-reasoner')).toBe(128_000);
    expect(getModelContextWindow('qwen-2.5-72b-instruct')).toBe(128_000);

    // Custom limit override
    expect(getModelContextWindow('kimi-k3.1-pro', 32_000)).toBe(32_000);
    expect(getModelContextWindow('unknown-model', 512_000)).toBe(512_000);
  });
});
