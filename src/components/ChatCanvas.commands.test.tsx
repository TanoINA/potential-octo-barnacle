import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import type { ComponentProps } from 'react';
import { ChatCanvas } from './ChatCanvas';
import type { AgentSettings } from '../types/agent';

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./AetherLogo', () => ({ AetherLogo: () => <span>Aether</span> }));
vi.mock('./MessageRow', () => ({ MessageRow: () => null }));

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
});

type CanvasProps = ComponentProps<typeof ChatCanvas>;

function agentSettings(models: string[]): AgentSettings {
  return {
    providers: [{
      id: 'p1', name: 'OpenAI', kind: 'openai', baseUrl: 'https://api.openai.com/v1',
      apiKey: 'test', model: models[0], models,
    }],
    activeProviderId: 'p1',
    systemPrompt: 'test',
    temperature: 0.7,
    streamResponse: true,
    thinking: false,
    reasoningEffort: 'medium',
    favoriteModels: [],
    skillPaths: [],
    mcpServers: [],
    maxTurns: 24,
    self_improvement_enabled: false,
  };
}

function renderCanvas(overrides: Partial<CanvasProps> = {}) {
  const props = {
    messages: [],
    isStreaming: false,
    stopRequested: false,
    onSendMessage: vi.fn(async () => true),
    onStop: vi.fn(),
    onSelectArtifact: vi.fn(),
    onClearSession: vi.fn(),
    onNewSession: () => 'new',
    onOpenSettings: vi.fn(),
    onOpenModelPicker: vi.fn(),
    effectiveConfig: null,
    sessionTitle: 'Command test',
    artifactCount: 0,
    isArtifactOpen: false,
    onToggleArtifacts: vi.fn(),
    onListFiles: async () => [],
    ...overrides,
  } as CanvasProps;
  render(<ChatCanvas {...props} />);
  return props;
}

describe('ChatCanvas streaming commands', () => {
  it('runs /stop while a turn is still streaming', async () => {
    const onStop = vi.fn();
    renderCanvas({ isStreaming: true, onStop });
    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: '/stop' } });
    // Dismiss the slash menu the way a user would, then submit with Enter.
    fireEvent.keyDown(input, { key: 'Escape' });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(onStop).toHaveBeenCalledOnce());
  });
});

describe('ChatCanvas council candidates', () => {
  it('disables Council Mode when no second model is configured', async () => {
    renderCanvas({ settings: agentSettings(['gpt-4o']) });
    const toggle = screen.getByTitle(/Council Mode/) as HTMLButtonElement;
    expect(toggle.disabled).toBe(true);
  });

  it('enables Council Mode when a second model exists', async () => {
    const settings = agentSettings(['gpt-4o', 'gpt-4o-mini']);
    renderCanvas({
      settings,
      effectiveConfig: { provider: settings.providers[0], thinking: false, reasoningEffort: 'medium', usesSettingsDefault: true },
    });
    const toggle = screen.getByTitle('Council Mode (Dual Model A/B Test)') as HTMLButtonElement;
    expect(toggle.disabled).toBe(false);
    fireEvent.click(toggle);
    expect(await screen.findByText('Dual Model Comparison')).toBeTruthy();
    const selects = screen.getAllByRole('combobox') as HTMLSelectElement[];
    expect(selects).toHaveLength(2);
    expect(selects[0].value).not.toBe(selects[1].value);
  });
});

describe('ChatCanvas prompt queueing', () => {
  it('queues a message when submitted while streaming and shows queue banner', async () => {
    const onSendMessage = vi.fn(async () => true);
    renderCanvas({ isStreaming: true, onSendMessage });

    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: 'Analyze performance bottlenecks' } });

    // The Queue button should be visible when streaming with text in input
    const queueBtn = screen.getByRole('button', { name: /Queue message/i });
    expect(queueBtn).toBeTruthy();
    fireEvent.click(queueBtn);

    // Input should be cleared
    expect((input as HTMLTextAreaElement).value).toBe('');
    // onSendMessage should not have been called yet
    expect(onSendMessage).not.toHaveBeenCalled();

    // Queue banner should appear
    expect(screen.getByTestId('queued-prompts-banner')).toBeTruthy();
    expect(screen.getByText('Analyze performance bottlenecks')).toBeTruthy();
    expect(screen.getByText('#1')).toBeTruthy();
  });

  it('allows editing a queued prompt back into the composer', async () => {
    renderCanvas({ isStreaming: true });

    const input = screen.getByRole('textbox') as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: 'First instruction to edit' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(screen.getByText('First instruction to edit')).toBeTruthy();
    expect(input.value).toBe('');

    // Click Edit button
    const editBtn = screen.getByRole('button', { name: /Edit/i });
    fireEvent.click(editBtn);

    // Prompt restored to composer and removed from queue banner
    expect(input.value).toBe('First instruction to edit');
    expect(screen.queryByTestId('queued-prompts-banner')).toBeNull();
  });

  it('allows removing a queued prompt', async () => {
    renderCanvas({ isStreaming: true });

    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: 'Cancel this instruction' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(screen.getByText('Cancel this instruction')).toBeTruthy();

    const removeBtn = screen.getByRole('button', { name: /Remove queued prompt/i });
    fireEvent.click(removeBtn);

    expect(screen.queryByTestId('queued-prompts-banner')).toBeNull();
  });

  it('automatically dispatches queued prompt when streaming ends', async () => {
    const onSendMessage = vi.fn(async () => true);
    const { rerender } = render(<ChatCanvas {...{
      messages: [],
      isStreaming: true,
      stopRequested: false,
      onSendMessage,
      onStop: vi.fn(),
      onSelectArtifact: vi.fn(),
      onClearSession: vi.fn(),
      onNewSession: () => 'new',
      onOpenSettings: vi.fn(),
      onOpenModelPicker: vi.fn(),
      effectiveConfig: null,
      sessionTitle: 'Command test',
      artifactCount: 0,
      isArtifactOpen: false,
      onToggleArtifacts: vi.fn(),
      onListFiles: async () => [],
    } as any} />);

    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: 'Next prompt to run after stream' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(onSendMessage).not.toHaveBeenCalled();

    // Now stream completes (isStreaming becomes false)
    rerender(<ChatCanvas {...{
      messages: [],
      isStreaming: false,
      stopRequested: false,
      onSendMessage,
      onStop: vi.fn(),
      onSelectArtifact: vi.fn(),
      onClearSession: vi.fn(),
      onNewSession: () => 'new',
      onOpenSettings: vi.fn(),
      onOpenModelPicker: vi.fn(),
      effectiveConfig: null,
      sessionTitle: 'Command test',
      artifactCount: 0,
      isArtifactOpen: false,
      onToggleArtifacts: vi.fn(),
      onListFiles: async () => [],
    } as any} />);

    await waitFor(() => {
      expect(onSendMessage).toHaveBeenCalledWith('Next prompt to run after stream', undefined);
    });
  });

  it('renders execution mode pill and allows selecting different execution modes', async () => {
    localStorage.removeItem('aether_execution_mode');
    const onSendMessage = vi.fn(async () => true);
    renderCanvas({ onSendMessage });

    const modeBtn = screen.getByLabelText('Execution mode');
    expect(modeBtn).toBeTruthy();
    expect(modeBtn.textContent).toContain('Turbo');

    // Open execution mode menu
    fireEvent.click(modeBtn);
    expect(screen.getByText('Request Review')).toBeTruthy();
    expect(screen.getByText('Every command requires approval.')).toBeTruthy();
    expect(screen.getByText('Default')).toBeTruthy();
    expect(screen.getByText('Auto (Preview)')).toBeTruthy();

    // Select Request Review
    fireEvent.click(screen.getByText('Request Review'));
    expect(modeBtn.textContent).toContain('Request Review');

    // Type a prompt and send
    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: 'Test prompt' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(onSendMessage).toHaveBeenCalledWith('Test prompt', undefined, { executionMode: 'request_review' });
    });
  });
});
