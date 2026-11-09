import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
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

describe('ChatCanvas review commands', () => {
  it.each(['/refine', '/review', '/reflect'])('runs bare %s locally and describes the full conversation', async (command) => {
    const onReviewSession = vi.fn(async () => true);
    const props = renderCanvas({ onReviewSession });
    const input = screen.getByLabelText('Message AetherAgent');
    fireEvent.change(input, { target: { value: command } });
    expect(screen.getAllByText(/Review the full conversation/).length).toBeGreaterThan(0);
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(onReviewSession).toHaveBeenCalledOnce());
    expect(props.onSendMessage).not.toHaveBeenCalled();
  });

  it.each(['/refine explain', '/review explain', '/reflect explain'])('sends non-bare %s as a normal prompt', async (command) => {
    const onReviewSession = vi.fn(async () => true);
    const props = renderCanvas({ onReviewSession });
    const input = screen.getByLabelText('Message AetherAgent');
    fireEvent.change(input, { target: { value: command } });
    fireEvent.keyDown(input, { key: 'Escape' });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(props.onSendMessage).toHaveBeenCalledWith(command, undefined, expect.any(Object)));
    expect(onReviewSession).not.toHaveBeenCalled();
  });
});
describe('ChatCanvas plan commands', () => {
  it.each(['enter', 'button'])('sends /plan with its goal using %s', async (method) => {
    const props = renderCanvas();
    const input = screen.getByLabelText('Message AetherAgent');
    fireEvent.change(input, { target: { value: '/plan Build a parser' } });
    if (method === 'enter') fireEvent.keyDown(input, { key: 'Enter' });
    else fireEvent.click(screen.getByRole('button', { name: 'Send message' }));
    await waitFor(() => expect(props.onSendMessage).toHaveBeenCalledWith(
      '/plan Build a parser', undefined, expect.objectContaining({ executionMode: expect.any(String) }),
    ));
  });

  it('keeps a rejected /plan draft for correction', async () => {
    renderCanvas({ onSendMessage: vi.fn(async () => false) });
    const input = screen.getByLabelText('Message AetherAgent') as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: '/plan Build a parser' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }));
    await waitFor(() => expect(input.value).toBe('/plan Build a parser'));
  });
});

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

describe('ChatCanvas queue dispatch contract', () => {
  function queuedCanvas(extra: Partial<CanvasProps> = {}) {
    const settings = agentSettings(['model-a', 'model-b']);
    const props: CanvasProps = {
      messages: [], isStreaming: true, stopRequested: false,
      onSendMessage: vi.fn(async () => true), onStop: vi.fn(), onSelectArtifact: vi.fn(),
      onClearSession: vi.fn(), onNewSession: () => 'next', onOpenSettings: vi.fn(), onOpenModelPicker: vi.fn(),
      effectiveConfig: { provider: settings.providers[0], thinking: false, reasoningEffort: 'medium', usesSettingsDefault: true },
      settings, sessionTitle: 'Queue test', currentSessionId: 'session-a', artifactCount: 0,
      isArtifactOpen: false, onToggleArtifacts: vi.fn(), onListFiles: async () => [], ...extra,
    };
    return { props, ...render(<ChatCanvas {...props} />) };
  }

  function enqueue(text: string) {
    fireEvent.change(screen.getByLabelText('Message AetherAgent'), { target: { value: text } });
    fireEvent.click(screen.getByRole('button', { name: 'Queue message' }));
  }

  it('preserves the queued execution mode and Council candidates until accepted', async () => {
    localStorage.setItem('aether_execution_mode', 'request_review');
    let accept!: (accepted: boolean) => void;
    const send = vi.fn(() => new Promise<boolean>((resolve) => { accept = resolve; }));
    const { props, rerender } = queuedCanvas({ onSendMessage: send });
    fireEvent.click(screen.getByTitle('Council Mode (Dual Model A/B Test)'));
    enqueue('compare this after the run');
    rerender(<ChatCanvas {...props} isStreaming={false} />);
    await waitFor(() => expect(send).toHaveBeenCalledOnce());
    expect(send).toHaveBeenCalledWith('compare this after the run', undefined, expect.objectContaining({
      executionMode: 'request_review', council: true,
      candidateA: expect.objectContaining({ provider: expect.objectContaining({ model: 'model-a' }) }),
      candidateB: expect.objectContaining({ provider: expect.objectContaining({ model: 'model-b' }) }),
    }));
    expect(screen.getByTestId('queued-prompts-banner')).toBeTruthy();
    await act(async () => accept(true));
    expect(screen.queryByTestId('queued-prompts-banner')).toBeNull();
  });

  it('retains a rejected item and offers explicit retry instead of spinning', async () => {
    const send = vi.fn(async () => false);
    const { props, rerender } = queuedCanvas({ onSendMessage: send });
    enqueue('must not be lost');
    rerender(<ChatCanvas {...props} isStreaming={false} />);
    await waitFor(() => expect(send).toHaveBeenCalledOnce());
    expect(screen.getByText('must not be lost')).toBeTruthy();
    fireEvent.click(await screen.findByRole('button', { name: 'Resume Queue' }));
    await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
    expect(screen.getByText('must not be lost')).toBeTruthy();
  });

  it('does not dispatch the old session queue on navigation to an idle session', async () => {
    const send = vi.fn(async () => true);
    const { props, rerender } = queuedCanvas({ onSendMessage: send });
    enqueue('belongs to session A');
    rerender(<ChatCanvas {...props} currentSessionId="session-b" isStreaming={false} />);
    await act(async () => {});
    expect(send).not.toHaveBeenCalled();
    expect(screen.queryByTestId('queued-prompts-banner')).toBeNull();
  });

  it('restores each session queue after switching chats without dispatching into the other chat', async () => {
    const send = vi.fn(async () => true);
    const { props, rerender } = queuedCanvas({ onSendMessage: send });
    enqueue('belongs to session A');
    rerender(<ChatCanvas {...props} currentSessionId="session-b" />);
    expect(screen.queryByTestId('queued-prompts-banner')).toBeNull();
    enqueue('belongs to session B');
    rerender(<ChatCanvas {...props} />);
    expect(screen.getByText('belongs to session A')).toBeTruthy();
    expect(screen.queryByText('belongs to session B')).toBeNull();
    expect(send).not.toHaveBeenCalled();
    rerender(<ChatCanvas {...props} isStreaming={false} />);
    await waitFor(() => expect(send).toHaveBeenCalledOnce());
    expect(send).toHaveBeenCalledWith('belongs to session A', undefined, expect.any(Object));
    rerender(<ChatCanvas {...props} currentSessionId="session-b" />);
    expect(screen.getByText('belongs to session B')).toBeTruthy();
  });

  it('removes only the viewed session item after switching chats', () => {
    const { props, rerender } = queuedCanvas();
    enqueue('A stays queued');
    rerender(<ChatCanvas {...props} currentSessionId="session-b" />);
    enqueue('B can be removed');
    fireEvent.click(screen.getByRole('button', { name: 'Remove queued prompt' }));
    expect(screen.queryByTestId('queued-prompts-banner')).toBeNull();
    rerender(<ChatCanvas {...props} />);
    expect(screen.getByText('A stays queued')).toBeTruthy();
  });

  it.each([true, false])('settles pending acceptance %s against its origin even after navigation', async (accepted) => {
    let settle!: (value: boolean) => void;
    const send = vi.fn(() => new Promise<boolean>((resolve) => { settle = resolve; }));
    const { props, rerender } = queuedCanvas({ onSendMessage: send });
    enqueue('pending A');
    rerender(<ChatCanvas {...props} isStreaming={false} />);
    expect(send).toHaveBeenCalledOnce();
    rerender(<ChatCanvas {...props} currentSessionId="session-b" />);
    enqueue('pending B');
    await act(async () => settle(accepted));
    expect(screen.getByText('pending B')).toBeTruthy();
    rerender(<ChatCanvas {...props} isStreaming={false} />);
    expect(send).toHaveBeenCalledOnce();
    if (accepted) expect(screen.queryByTestId('queued-prompts-banner')).toBeNull();
    else {
      expect(screen.getByText('pending A')).toBeTruthy();
      expect(screen.getByRole('button', { name: 'Resume Queue' })).toBeTruthy();
    }
  });

  it('preserves Stop pause across navigation and waits for loaded history before resume', async () => {
    const send = vi.fn(async () => true);
    const { props, rerender } = queuedCanvas({ onSendMessage: send });
    enqueue('paused A');
    rerender(<ChatCanvas {...props} stopRequested />);
    rerender(<ChatCanvas {...props} currentSessionId="session-b" isStreaming={false} />);
    rerender(<ChatCanvas {...props} isStreaming={false} isSessionLoading />);
    expect(screen.getByRole('button', { name: 'Resume Queue' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Resume Queue' }));
    expect(send).not.toHaveBeenCalled();
    rerender(<ChatCanvas {...props} isStreaming={false} />);
    await waitFor(() => expect(send).toHaveBeenCalledOnce());
  });

  it('does not consume a new composer draft when dispatching a queued local command', async () => {
    const onOpenModelPicker = vi.fn();
    const { props, rerender } = queuedCanvas({ onOpenModelPicker });
    enqueue('/model');
    const input = screen.getByLabelText('Message AetherAgent') as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: 'new draft for later' } });
    rerender(<ChatCanvas {...props} isStreaming={false} />);
    await waitFor(() => expect(onOpenModelPicker).toHaveBeenCalledOnce());
    expect(input.value).toBe('new draft for later');
  });

  it('does not dispatch after Stop until Resume is selected', async () => {
    const send = vi.fn(async () => true);
    const { props, rerender } = queuedCanvas({ onSendMessage: send });
    enqueue('paused instruction');
    rerender(<ChatCanvas {...props} stopRequested />);
    rerender(<ChatCanvas {...props} isStreaming={false} stopRequested={false} />);
    await act(async () => {});
    expect(send).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Resume Queue' }));
    await waitFor(() => expect(send).toHaveBeenCalledOnce());
  });

  it('waits for review to finish then dispatches a local command locally', async () => {
    const onNewSession = vi.fn(() => 'next');
    const { props, rerender } = queuedCanvas({ onNewSession });
    enqueue('/new');
    rerender(<ChatCanvas {...props} isStreaming={false} isReviewing />);
    expect(onNewSession).not.toHaveBeenCalled();
    rerender(<ChatCanvas {...props} isStreaming={false} isReviewing={false} />);
    await waitFor(() => expect(onNewSession).toHaveBeenCalledOnce());
    expect(props.onSendMessage).not.toHaveBeenCalled();
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
      expect(onSendMessage).toHaveBeenCalledWith('Next prompt to run after stream', undefined, expect.objectContaining({ executionMode: expect.any(String) }));
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
    expect(screen.getByText('Write, command, and MCP tools are blocked.')).toBeTruthy();
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
