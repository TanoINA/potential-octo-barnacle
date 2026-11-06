import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSettings, EffectiveRunConfig, Session, SessionData } from './types/agent';

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
});

const mocks = vi.hoisted(() => ({
  agent: { current: null as unknown as Record<string, unknown> | null },
}));

vi.mock('./hooks/useAgent', () => ({ useAgent: () => mocks.agent.current }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('./components/AetherLogo', () => ({ AetherLogo: () => <span>Aether</span> }));
vi.mock('./components/MessageRow', () => ({ MessageRow: () => null }));

const settings: AgentSettings = {
  providers: [{ id: 'p1', name: 'OpenAI', kind: 'openai', baseUrl: 'https://api.openai.com/v1', apiKey: 'test', model: 'gpt-4o', models: ['gpt-4o', 'gpt-4o-mini'] }],
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

const effective: EffectiveRunConfig = {
  provider: settings.providers[0],
  thinking: false,
  reasoningEffort: 'medium',
  usesSettingsDefault: true,
};

const session: Session = { id: 'sess-1', title: 'Test session', createdAt: 0, updatedAt: 0, messageCount: 0, workspaceCwd: '' };

function sessionData(): SessionData {
  return { session, messages: [], artifacts: [], modelOverride: null };
}

function makeAgent(overrides: Record<string, unknown> = {}) {
  return {
    messages: [],
    artifacts: [],
    activeArtifact: null,
    activeArtifactId: null,
    setActiveArtifactId: vi.fn(),
    subagents: [],
    activeSkills: [],
    listSkills: vi.fn(async () => []),
    listAllArtifacts: vi.fn(async () => []),
    deepSearch: vi.fn(async () => []),
    isStreaming: false,
    isReviewing: false,
    reviewSession: vi.fn(async () => true),
    stopRequested: false,
    isLoading: false,
    sessions: [],
    currentSessionId: session.id,
    sessionTitle: session.title,
    modelOverride: null,
    settings,
    workspaceCwd: '',
    effectiveConfig: effective,
    lastError: null,
    clearError: vi.fn(),
    sendMessage: vi.fn(async () => true),
    selectCouncilWinner: vi.fn(async () => true),
    stopAgent: vi.fn(),
    compactSession: vi.fn(async () => sessionData()),
    undoTurn: vi.fn(async () => null),
    initAgentsMd: vi.fn(async () => 'AGENTS.md'),
    readAgentsMd: vi.fn(async () => null),
    saveAgentsMd: vi.fn(async () => undefined),
    newSession: vi.fn(() => 'sess-new'),
    switchSession: vi.fn(async () => sessionData()),
    deleteSession: vi.fn(async () => undefined),
    renameSession: vi.fn(async () => sessionData()),
    togglePinSession: vi.fn(async () => session),
    exportSession: vi.fn(async () => undefined),
    clearCurrentSession: vi.fn(async () => 'sess-1'),
    updateSettings: vi.fn(async () => undefined),
    selectModel: vi.fn(async () => undefined),
    updateRunConfig: vi.fn(async () => undefined),
    clearModelOverride: vi.fn(async () => undefined),
    toggleFavorite: vi.fn(async () => undefined),
    addCustomModel: vi.fn(async () => undefined),
    refreshModels: vi.fn(async () => undefined),
    selectWorkspace: vi.fn(async () => ''),
    clearWorkspace: vi.fn(async () => undefined),
    listWorkspaceFiles: vi.fn(async () => []),
    ...overrides,
  };
}

async function runSlashCommand(command: string) {
  const input = screen.getByLabelText('Message AetherAgent');
  fireEvent.change(input, { target: { value: command } });
  const option = await screen.findByText(command, { selector: '[data-slash-item] *' });
  fireEvent.click(option);
}

function setNarrowViewport(narrow: boolean) {
  Object.defineProperty(window, 'matchMedia', {
    value: vi.fn().mockImplementation((query: string) => ({
      matches: narrow,
      media: query,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    })),
    configurable: true,
  });
}

describe('App command wiring', () => {
  beforeEach(() => {
    setNarrowViewport(false);
  });

  it('routes /review to the reflection reviewer instead of a chat completion', async () => {
    const agent = makeAgent();
    mocks.agent.current = agent;
    const { default: App } = await import('./App');
    render(<App />);
    await runSlashCommand('/review');
    await waitFor(() => expect(agent.reviewSession).toHaveBeenCalledOnce());
    expect(agent.sendMessage).not.toHaveBeenCalled();
  }, 20000);

  it('routes /compact to the session compactor', async () => {
    const agent = makeAgent();
    mocks.agent.current = agent;
    const { default: App } = await import('./App');
    render(<App />);
    await runSlashCommand('/compact');
    await waitFor(() => expect(agent.compactSession).toHaveBeenCalledOnce());
    expect(agent.sendMessage).not.toHaveBeenCalled();
  }, 20000);

  it('shows the reviewing status while a reflection review runs', async () => {
    const agent = makeAgent({ isReviewing: true });
    mocks.agent.current = agent;
    const { default: App } = await import('./App');
    render(<App />);
    expect(await screen.findByText('Reviewing…')).toBeTruthy();
  });
});

function sidebarToggle() {
  return screen.queryByTitle(/sidebar/i) as HTMLButtonElement | null;
}

describe('App sidebar layout', () => {
  beforeEach(() => {
    setNarrowViewport(false);
  });

  it('renders a sidebar toggle that collapses and restores the sidebar', async () => {
    mocks.agent.current = makeAgent();
    const { default: App } = await import('./App');
    render(<App />);
    expect(document.querySelector('aside')).toBeTruthy();
    fireEvent.click(sidebarToggle()!);
    await waitFor(() => expect(document.querySelector('aside')).toBeNull());
    expect(sidebarToggle()?.title).toMatch(/Show sidebar/i);
    fireEvent.click(sidebarToggle()!);
    await waitFor(() => expect(document.querySelector('aside')).toBeTruthy());
    expect(sidebarToggle()?.title).toMatch(/Hide sidebar/i);
  });

  it('toggles the sidebar with Ctrl+B', async () => {
    mocks.agent.current = makeAgent();
    const { default: App } = await import('./App');
    render(<App />);
    fireEvent.keyDown(window, { key: 'b', ctrlKey: true });
    await waitFor(() => expect(document.querySelector('aside')).toBeNull());
    fireEvent.keyDown(window, { key: 'b', ctrlKey: true });
    await waitFor(() => expect(document.querySelector('aside')).toBeTruthy());
  });

  it('overlays the sidebar on narrow viewports and closes it via the backdrop', async () => {
    setNarrowViewport(true);
    mocks.agent.current = makeAgent();
    const { default: App } = await import('./App');
    render(<App />);
    // Narrow viewports start with the sidebar closed so the chat is usable immediately.
    await waitFor(() => expect(document.querySelector('aside')).toBeNull());
    fireEvent.click(sidebarToggle()!);
    const backdrop = await screen.findByRole('button', { name: 'Close sidebar' });
    fireEvent.click(backdrop);
    await waitFor(() => expect(document.querySelector('aside')).toBeNull());
    expect(screen.queryByRole('button', { name: 'Close sidebar' })).toBeNull();
  });
});

describe('App modal & panel wiring', () => {
  beforeEach(() => {
    setNarrowViewport(false);
  });

  it('opens the session history modal for /history', async () => {
    mocks.agent.current = makeAgent();
    const { default: App } = await import('./App');
    render(<App />);
    await runSlashCommand('/history');
    expect(await screen.findByRole('dialog', { name: 'Session history and context' })).toBeTruthy();
  }, 20000);

  it('opens the tools inspector modal for /tools', async () => {
    mocks.agent.current = makeAgent();
    const { default: App } = await import('./App');
    render(<App />);
    await runSlashCommand('/tools');
    expect(await screen.findByRole('dialog', { name: 'Active capabilities and tools' })).toBeTruthy();
  }, 20000);

  it('opens the workspace memory modal for /memory', async () => {
    mocks.agent.current = makeAgent();
    const { default: App } = await import('./App');
    render(<App />);
    await runSlashCommand('/memory');
    expect(await screen.findByRole('dialog', { name: 'Workspace memory and instructions' })).toBeTruthy();
  }, 20000);

  it('opens the spotlight bar from the header more-menu', async () => {
    mocks.agent.current = makeAgent();
    const { default: App } = await import('./App');
    render(<App />);
    fireEvent.click(screen.getByTitle('More actions (Search, Export, Settings)'));
    fireEvent.click(await screen.findByRole('button', { name: /^Search/ }));
    expect(await screen.findByRole('dialog', { name: 'Spotlight command prompt' })).toBeTruthy();
  }, 20000);

  it('opens the live shell panel from the header', async () => {
    mocks.agent.current = makeAgent();
    const { default: App } = await import('./App');
    render(<App />);
    fireEvent.click(screen.getByTitle('Open Live Shell terminal'));
    expect(await screen.findByText('Live Shell')).toBeTruthy();
  }, 20000);
});

describe('App council and composer wiring', () => {
  beforeEach(() => {
    setNarrowViewport(false);
  });

  it('passes settings through so Council Mode sends a dual-model run', async () => {
    const agent = makeAgent();
    mocks.agent.current = agent;
    const { default: App } = await import('./App');
    render(<App />);
    fireEvent.click(screen.getByTitle('Council Mode (Dual Model A/B Test)'));
    const input = screen.getByLabelText('Message AetherAgent');
    fireEvent.change(input, { target: { value: 'compare both models' } });
    fireEvent.click(screen.getByRole('button', { name: 'Send message' }));
    await waitFor(() => expect(agent.sendMessage).toHaveBeenCalledOnce());
    expect((agent.sendMessage.mock.calls[0] as unknown[])[2]).toEqual(expect.objectContaining({ council: true }));
  }, 20000);

  it('wires the composer thinking switch to updateRunConfig', async () => {
    const agent = makeAgent();
    mocks.agent.current = agent;
    const { default: App } = await import('./App');
    render(<App />);
    fireEvent.click(screen.getByTitle('Model & Thinking controls'));
    fireEvent.click(screen.getByRole('switch'));
    await waitFor(() => expect(agent.updateRunConfig).toHaveBeenCalledWith({ thinking: true }));
  }, 20000);
});
