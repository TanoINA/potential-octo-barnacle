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
