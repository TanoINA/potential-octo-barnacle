import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentSettings, Artifact, EffectiveRunConfig, Session, SessionData } from '../types/agent';
import App from '../App';
import { Sidebar } from './Sidebar';
import { ChatCanvas } from './ChatCanvas';
import { setTheme } from '../themes';

const mocks = vi.hoisted(() => ({
  agent: { current: null as unknown as Record<string, unknown> | null },
  invoke: vi.fn(async (): Promise<unknown> => undefined),
  listen: vi.fn(async () => vi.fn()),
}));

vi.mock('@tauri-apps/api/core', async (importOriginal) => ({
  ...await importOriginal<typeof import('@tauri-apps/api/core')>(),
  invoke: mocks.invoke,
}));

vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({ onDragDropEvent: mocks.listen }),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('../hooks/useAgent', () => ({ useAgent: () => mocks.agent.current }));
vi.mock('./AetherLogo', () => ({ AetherLogo: () => <span>Aether</span> }));
vi.mock('./MessageRow', () => ({ MessageRow: () => <div data-testid="mock-msg" /> }));

const settings: AgentSettings = {
  providers: [{
    id: 'p1',
    name: 'OpenAI',
    kind: 'openai',
    baseUrl: 'https://api.openai.com/v1',
    apiKey: 'test',
    model: 'gpt-4o',
    models: ['gpt-4o'],
  }],
  activeProviderId: 'p1',
  systemPrompt: 'test',
  temperature: 0.7,
  streamResponse: true,
  thinking: false,
  reasoningEffort: 'medium',
  favoriteModels: [],
  skillPaths: [],
  mcpServers: [{ id: 'mcp-1', name: 'fs', command: 'npx', args: ['-y', 'server'], env: {}, enabled: true }],
  maxTurns: 24,
  self_improvement_enabled: false,
};

const effective: EffectiveRunConfig = {
  provider: settings.providers[0],
  thinking: false,
  reasoningEffort: 'medium',
  usesSettingsDefault: true,
};

const sampleArtifact: Artifact = {
  id: 'art-1',
  title: 'Example Script',
  type: 'code',
  content: 'console.log("ok");',
  timestamp: 1,
};

const baseSession: Session = {
  id: 'sess-1',
  title: 'Docked test',
  createdAt: 0,
  updatedAt: 0,
  messageCount: 1,
  workspaceCwd: 'D:\\Projects\\Alpha',
};

function makeSessionData(): SessionData {
  return { session: baseSession, messages: [], artifacts: [sampleArtifact], modelOverride: null };
}

function makeAgent(overrides: Record<string, unknown> = {}) {
  return {
    messages: [],
    artifacts: [sampleArtifact],
    activeArtifact: sampleArtifact,
    activeArtifactId: sampleArtifact.id,
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
    sessions: [baseSession],
    currentSessionId: baseSession.id,
    sessionTitle: baseSession.title,
    modelOverride: null,
    settings,
    workspaceCwd: 'D:\\Projects\\Alpha',
    effectiveConfig: effective,
    lastError: null,
    clearError: vi.fn(),
    sendMessage: vi.fn(async () => true),
    selectCouncilWinner: vi.fn(async () => true),
    stopAgent: vi.fn(),
    compactSession: vi.fn(async () => makeSessionData()),
    undoTurn: vi.fn(async () => null),
    initAgentsMd: vi.fn(async () => 'AGENTS.md'),
    readAgentsMd: vi.fn(async () => null),
    saveAgentsMd: vi.fn(async () => undefined),
    newSession: vi.fn(() => 'sess-new'),
    switchSession: vi.fn(async () => makeSessionData()),
    deleteSession: vi.fn(async () => undefined),
    renameSession: vi.fn(async () => makeSessionData()),
    togglePinSession: vi.fn(async () => baseSession),
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

beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', { configurable: true, value: vi.fn() });
});

beforeEach(() => {
  localStorage.clear();
  setTheme('nous-dark');
});

afterEach(async () => {
  await act(async () => {
    cleanup();
  });
});

describe('Fase 2 Shell Architecture', () => {
  it('groups recent sessions by workspace and allows collapsing each project group', () => {
    const sessions: Session[] = [
      { id: 's-alpha', title: 'Alpha bugfix', createdAt: 100, updatedAt: 300, messageCount: 3, workspaceCwd: 'D:\\Work\\Alpha' },
      { id: 's-beta', title: 'Beta release', createdAt: 90, updatedAt: 200, messageCount: 2, workspaceCwd: 'D:\\Work\\Beta' },
    ];

    render(
      <Sidebar
        sessions={sessions}
        currentSessionId="s-alpha"
        onNewSession={vi.fn()}
        onSelectSession={vi.fn()}
        onDeleteSession={vi.fn()}
        onRenameSession={vi.fn()}
        onExportSession={vi.fn()}
        onOpenSettings={vi.fn()}
        workspaceCwd="D:\\Work\\Alpha"
        onSelectWorkspace={vi.fn()}
        effectiveConfig={effective}
      />,
    );

    const alphaGroup = screen
      .getAllByRole('button', { name: /Alpha/i })
      .find((btn) => btn.hasAttribute('aria-expanded'))!;
    expect(alphaGroup).toBeTruthy();
    expect(alphaGroup.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('Alpha bugfix')).toBeTruthy();
    expect(screen.getByText('Beta release')).toBeTruthy();

    fireEvent.click(alphaGroup);
    expect(alphaGroup.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByText('Alpha bugfix')).toBeNull();
    expect(screen.getByText('Beta release')).toBeTruthy();

    fireEvent.click(alphaGroup);
    expect(alphaGroup.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByText('Alpha bugfix')).toBeTruthy();
  });

  it('docks ArtifactPane on desktop with a keyboard-resizable separator sash', async () => {
    Object.defineProperty(window, 'matchMedia', {
      configurable: true,
      value: vi.fn().mockImplementation((query: string) => ({
        matches: false,
        media: query,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      })),
    });

    mocks.agent.current = makeAgent();
    render(<App />);

    fireEvent.click(screen.getByTitle('Show task inspector / artifacts panel'));

    const sash = await screen.findByRole('separator', { name: 'Resize side pane' }, { timeout: 5000 });
    const artifactSelect = await screen.findByLabelText('Select artifact', {}, { timeout: 5000 });
    const aside = artifactSelect.closest('aside');
    expect(aside).toBeTruthy();
    expect(aside?.className).toContain('relative');
    expect(aside?.className).not.toContain('fixed');
    expect(aside?.style.width).toBe('420px');
    expect(sash.getAttribute('aria-valuenow')).toBe('420');
    expect(sash.getAttribute('aria-valuemin')).toBe('280');
    expect(sash.getAttribute('aria-valuemax')).toBe('720');

    fireEvent.mouseDown(sash, { button: 2 });
    expect(document.body.style.cursor).toBe('');

    fireEvent.keyDown(sash, { key: 'ArrowLeft' });
    expect(aside?.style.width).toBe('444px');
    expect(sash.getAttribute('aria-valuenow')).toBe('444');
    expect(localStorage.getItem('aether.rightPaneWidth')).toBe('444');

    fireEvent.keyDown(sash, { key: 'ArrowRight' });
    expect(aside?.style.width).toBe('420px');
  });

  it('toggles the composer-docked Status Stack via the grip ridge and cycles theme from the bottom footer', () => {
    render(
      <ChatCanvas
        messages={[{ id: 'm1', role: 'user', content: 'Analyze workspace architecture cleanly', timestamp: 1 }]}
        isStreaming={false}
        stopRequested={false}
        onSendMessage={vi.fn(async () => true)}
        onStop={vi.fn()}
        onSelectArtifact={vi.fn()}
        onClearSession={vi.fn()}
        onNewSession={vi.fn(() => 'new')}
        onOpenSettings={vi.fn()}
        onOpenModelPicker={vi.fn()}
        effectiveConfig={effective}
        sessionTitle="Fase 2 check"
        artifactCount={0}
        isArtifactOpen={false}
        onToggleArtifacts={vi.fn()}
        onListFiles={vi.fn(async () => [])}
        settings={settings}
        currentSessionId="sess-1"
        workspaceCwd="D:\\Work\\AetherAgent"
        onOpenFolder={vi.fn()}
      />,
    );

    const grip = screen.getByTestId('status-stack-grip');
    expect(grip.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByTestId('status-stack-card')).toBeTruthy();
    expect(screen.getByTestId('context-meter')).toBeTruthy();

    fireEvent.click(grip);
    expect(grip.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByTestId('status-stack-card')).toBeNull();
    expect(screen.queryByTestId('context-meter')).toBeNull();

    fireEvent.click(grip);
    expect(grip.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByTestId('context-meter')).toBeTruthy();

    const footer = screen.getByTestId('workspace-status-bar');
    expect(footer.textContent).toContain('AetherAgent');
    expect(footer.textContent).toContain('1 MCP');
    expect(footer.textContent).toContain('0 skills');

    const skinBtn = screen.getByRole('button', { name: 'Cycle color skin' });
    expect(skinBtn.textContent).toBe('Nous');
    fireEvent.click(skinBtn);
    expect(skinBtn.textContent).toBe('Graphite');
    expect(document.documentElement.getAttribute('data-theme')).toBe('antigravity');
  });
});
