import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import type { AgentSettings, SkillInfo } from '../types/agent';
import { SettingsModal, parseEnv } from './SettingsModal';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('../utils/audioChime', () => ({ playTaskCompleteChime: vi.fn() }));

const baseSettings: AgentSettings = {
  providers: [{
    id: 'local',
    name: 'Local provider',
    kind: 'openai',
    baseUrl: 'http://localhost:8080/v1',
    apiKey: '',
    model: 'test-model',
  }],
  activeProviderId: 'local',
  systemPrompt: '',
  temperature: 0.7,
  streamResponse: true,
  thinking: false,
  reasoningEffort: 'medium',
  favoriteModels: [],
  skillPaths: [],
  maxTurns: 24,
  mcpServers: [],
  self_improvement_enabled: false,
};

function legacySettings(): AgentSettings {
  const legacy: Partial<AgentSettings> = { ...baseSettings };
  delete legacy.self_improvement_enabled;
  return legacy as AgentSettings;
}

beforeEach(() => { vi.mocked(invoke).mockReset().mockResolvedValue([]); });

describe('SettingsModal reflection opt-in', () => {
  it('preserves an explicit false preference when saving from Providers', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<SettingsModal settings={baseSettings} onSave={onSave} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(onSave).toHaveBeenCalledOnce());
    expect(onSave.mock.calls[0][0].self_improvement_enabled).toBe(false);
  });


  it('configures review cadence, approval, destination and auxiliary provider without mutating settings', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<SettingsModal settings={baseSettings} onSave={onSave} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));
    const interval = screen.getByRole('spinbutton', { name: 'Creation nudge interval' }) as HTMLInputElement;
    expect(interval.value).toBe('10');
    expect((screen.getByRole('checkbox', { name: 'Require approval before writing skills' }) as HTMLInputElement).checked).toBe(false);
    fireEvent.change(interval, { target: { value: '0' } });
    fireEvent.click(screen.getByRole('checkbox', { name: 'Require approval before writing skills' }));
    fireEvent.change(screen.getByRole('textbox', { name: 'Skill creation directory' }), { target: { value: 'D:/shared/skills' } });
    fireEvent.change(screen.getByRole('combobox', { name: 'Review provider' }), { target: { value: 'local' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(onSave).toHaveBeenCalledOnce());
    expect(onSave.mock.calls[0][0].reviewConfig).toEqual({ creationNudgeInterval: 0, writeApproval: true, createDir: 'D:/shared/skills', auxiliaryProviderId: 'local' });
    expect(baseSettings).not.toHaveProperty('reviewConfig');
  });
  it('saves an explicit boolean when opting in and back out without mutating the supplied settings', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const settings = structuredClone(baseSettings);
    render(<SettingsModal settings={settings} onSave={onSave} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));
    const toggle = screen.getByRole('checkbox', { name: 'Enable automatic self-improvement' }) as HTMLInputElement;
    fireEvent.click(toggle);
    expect(toggle.checked).toBe(true);
    expect(settings.self_improvement_enabled).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    expect(onSave.mock.calls[0][0].self_improvement_enabled).toBe(true);

    await waitFor(() => expect((screen.getByRole('button', { name: 'Save changes' }) as HTMLButtonElement).disabled).toBe(false));
    fireEvent.click(toggle);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
    expect(onSave.mock.calls[1][0].self_improvement_enabled).toBe(false);
    expect(invoke).not.toHaveBeenCalled();
  });

  it('reflects an already enabled preference and leaves the stored value alone on Cancel', () => {
    const onSave = vi.fn();
    const onClose = vi.fn();
    const settings = { ...baseSettings, self_improvement_enabled: true };
    render(<SettingsModal settings={settings} onSave={onSave} onClose={onClose} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));
    const toggle = screen.getByRole('checkbox', { name: 'Enable automatic self-improvement' }) as HTMLInputElement;
    expect(toggle.checked).toBe(true);
    fireEvent.click(toggle);
    expect(toggle.checked).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onSave).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
    expect(settings.self_improvement_enabled).toBe(true);
  });

  it('normalizes a missing legacy preference to explicit true when saving from Providers', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    render(<SettingsModal settings={legacySettings()} onSave={onSave} onClose={onClose} />);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(onSave).toHaveBeenCalledOnce());
    expect(onSave.mock.calls[0][0].self_improvement_enabled).toBe(true);
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  });

  it('discloses the provider request, local writes, and on-demand commands even when disabled', () => {
    render(<SettingsModal settings={baseSettings} onSave={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));

    const toggle = screen.getByRole('checkbox', { name: 'Enable automatic self-improvement' });
    const descriptionId = toggle.getAttribute('aria-describedby');
    expect(descriptionId).toBeTruthy();
    const disclosure = document.getElementById(descriptionId!);
    expect(disclosure?.textContent).toContain('extra request to the active provider');
    expect(disclosure?.textContent).toContain('creation nudge interval');
    expect(disclosure?.textContent).toContain('10 provider iterations');
    expect(disclosure?.textContent).not.toContain('10 successful runs');
    expect(disclosure?.textContent).toContain('No workspace is required');
    expect(disclosure?.textContent).toContain('approval');
    expect(disclosure?.textContent).toContain('/review');
    expect(disclosure?.textContent).toContain('/reflect');
    expect(disclosure?.textContent).toContain('even when automatic self-improvement is disabled');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('shows enabled-by-default automatic reflection for legacy settings in Agent defaults', () => {
    render(<SettingsModal settings={legacySettings()} onSave={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));

    expect(screen.getByRole('heading', { name: 'Agent Intelligence / Reflection' })).toBeTruthy();
    const toggle = screen.getByRole('checkbox', { name: 'Enable automatic self-improvement' });
    expect((toggle as HTMLInputElement).checked).toBe(true);
  });

  it('toggles and configures auto micro-compaction settings', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<SettingsModal settings={baseSettings} onSave={onSave} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));

    const toggle = screen.getByRole('checkbox', {
      name: 'Enable automatic micro-compaction of verbose tool outputs',
    }) as HTMLInputElement;
    expect(toggle.checked).toBe(true);

    const thresholdSelect = screen.getByRole('combobox', {
      name: 'Micro-compaction token threshold',
    }) as HTMLSelectElement;
    expect(thresholdSelect.value).toBe('65536');

    fireEvent.change(thresholdSelect, { target: { value: '32768' } });
    expect(thresholdSelect.value).toBe('32768');

    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(onSave).toHaveBeenCalledOnce());
    expect(onSave.mock.calls[0][0].autoCompactionEnabled).toBe(true);
    expect(onSave.mock.calls[0][0].autoCompactionThresholdTokens).toBe(32768);
  });
});

describe('SettingsModal skill detection', () => {
  it('ignores stale list_skills responses while skill roots are being edited', async () => {
    const resolvers: ((skills: SkillInfo[]) => void)[] = [];
    vi.mocked(invoke).mockImplementation(
      () => new Promise((resolve) => { resolvers.push(resolve as (skills: SkillInfo[]) => void); }),
    );

    render(<SettingsModal settings={baseSettings} onSave={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Skills & MCP' }));

    // First request (initial roots) is in flight.
    await waitFor(() => expect(resolvers.length).toBe(1));

    // Editing the roots textarea fires a second request.
    const rootsTextarea = screen.getByPlaceholderText('D:\\path\\to\\skills');
    fireEvent.change(rootsTextarea, { target: { value: 'C:/skills-a' } });
    await waitFor(() => expect(resolvers.length).toBe(2));

    const stale: SkillInfo = { name: 'stale-skill', description: '', path: 'old', source: 'workspace' };
    const fresh: SkillInfo = { name: 'fresh-skill', description: '', path: 'new', source: 'workspace' };

    // The newer response resolves first; the older one must not overwrite it.
    act(() => { resolvers[1]([fresh]); });
    act(() => { resolvers[0]([stale]); });

    await waitFor(() => expect(screen.getByText('fresh-skill')).toBeTruthy());
    expect(screen.queryByText('stale-skill')).toBeNull();
  });
});
it('opens About from Settings without saving settings', () => {
  const onAbout = vi.fn(); const onSave = vi.fn();
  render(<SettingsModal settings={baseSettings} onSave={onSave} onClose={vi.fn()} onAbout={onAbout} />);
  fireEvent.click(screen.getByRole('button', { name: 'About AetherAgent' }));
  expect(onAbout).toHaveBeenCalledOnce(); expect(onSave).not.toHaveBeenCalled();
});

describe('SettingsModal keyboard navigation', () => {
  it('uses an accessible tab pattern with arrow, Home, and End navigation', async () => {
    render(<SettingsModal settings={baseSettings} onSave={vi.fn()} onClose={vi.fn()} />);
    const providers = screen.getByRole('tab', { name: 'Providers' });
    const agent = screen.getByRole('tab', { name: 'Agent defaults' });
    const integrations = screen.getByRole('tab', { name: 'Skills & MCP' });
    expect(providers.getAttribute('aria-selected')).toBe('true');
    expect(screen.getByRole('tabpanel').getAttribute('aria-labelledby')).toBe('settings-tab-providers');
    providers.focus();
    fireEvent.keyDown(providers, { key: 'ArrowRight' });
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(resolve)); });
    expect(document.activeElement).toBe(agent);
    expect(agent.getAttribute('aria-selected')).toBe('true');
    expect(screen.getByRole('tabpanel').getAttribute('aria-labelledby')).toBe('settings-tab-agent');
    fireEvent.keyDown(agent, { key: 'ArrowRight' });
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(resolve)); });
    expect(document.activeElement).toBe(integrations);
    fireEvent.keyDown(integrations, { key: 'ArrowRight' });
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(resolve)); });
    expect(document.activeElement).toBe(providers);
    fireEvent.keyDown(providers, { key: 'End' });
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(resolve)); });
    expect(document.activeElement).toBe(integrations);
    fireEvent.keyDown(integrations, { key: 'Home' });
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(resolve)); });
    expect(document.activeElement).toBe(providers);
    fireEvent.keyDown(providers, { key: 'ArrowLeft' });
    await act(async () => { await new Promise((resolve) => requestAnimationFrame(resolve)); });
    expect(document.activeElement).toBe(integrations);
    expect(integrations.getAttribute('tabindex')).toBe('0');
    expect(providers.getAttribute('tabindex')).toBe('-1');
  });
});

describe('SettingsModal enhanced features', () => {
  it('toggles API key visibility between password and text', () => {
    render(<SettingsModal settings={baseSettings} onSave={vi.fn()} onClose={vi.fn()} />);
    const apiKeyInput = screen.getByPlaceholderText('Leave empty for local providers') as HTMLInputElement;
    expect(apiKeyInput.type).toBe('password');

    const toggleButton = screen.getByRole('button', { name: 'Show API key' });
    fireEvent.click(toggleButton);
    expect(apiKeyInput.type).toBe('text');

    const hideButton = screen.getByRole('button', { name: 'Hide API key' });
    fireEvent.click(hideButton);
    expect(apiKeyInput.type).toBe('password');
  });

  it('tests provider connection and displays discovery result', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'fetch_provider_models') return ['gpt-4o', 'gpt-4o-mini'];
      return [];
    });

    render(<SettingsModal settings={baseSettings} onSave={vi.fn()} onClose={vi.fn()} />);
    const testButton = screen.getByRole('button', { name: 'Test connection' });
    fireEvent.click(testButton);

    await waitFor(() => {
      expect(screen.getByText(/Connection successful: 2 model\(s\) discovered/i)).toBeTruthy();
    });
  });

  it('loads provider presets into the current form', () => {
    render(<SettingsModal settings={baseSettings} onSave={vi.fn()} onClose={vi.fn()} />);
    const presetSelect = screen.getByRole('combobox', { name: 'Load provider preset' }) as HTMLSelectElement;
    fireEvent.change(presetSelect, { target: { value: 'anthropic' } });

    const baseUrlInput = screen.getByDisplayValue('https://api.anthropic.com') as HTMLInputElement;
    expect(baseUrlInput).toBeTruthy();
  });

  it('configures default execution mode and resets agent defaults', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<SettingsModal settings={baseSettings} onSave={onSave} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));

    const modeSelect = screen.getByRole('combobox', { name: 'Default execution mode' }) as HTMLSelectElement;
    expect(modeSelect.value).toBe('turbo');

    fireEvent.change(modeSelect, { target: { value: 'auto' } });
    expect(modeSelect.value).toBe('auto');

    const resetButton = screen.getByRole('button', { name: 'Reset to defaults' });
    fireEvent.click(resetButton);
    expect(modeSelect.value).toBe('turbo');

    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(onSave).toHaveBeenCalledOnce());
    expect(onSave.mock.calls[0][0].executionMode).toBe('turbo');
  });

  it('browses folder to add a new skill root path', async () => {
    vi.mocked(open).mockResolvedValue('D:/custom/my-skills');

    render(<SettingsModal settings={baseSettings} onSave={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Skills & MCP' }));

    const browseBtn = screen.getByRole('button', { name: 'Browse folder' });
    fireEvent.click(browseBtn);

    await waitFor(() => {
      const textarea = screen.getByPlaceholderText('D:\\path\\to\\skills') as HTMLTextAreaElement;
      expect(textarea.value).toContain('D:/custom/my-skills');
    });
  });

  it('tests MCP server and reports discovered tools', async () => {
    vi.mocked(invoke).mockImplementation(async (cmd) => {
      if (cmd === 'list_mcp_tools') return [{ name: 'tool_one' }, { name: 'tool_two' }];
      return [];
    });

    const settingsWithMcp: AgentSettings = {
      ...baseSettings,
      mcpServers: [{
        id: 'test-mcp',
        name: 'Mock MCP',
        command: 'node',
        args: ['server.js'],
        env: {},
        enabled: true,
      }],
    };

    render(<SettingsModal settings={settingsWithMcp} onSave={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Skills & MCP' }));

    const testServerBtn = screen.getByRole('button', { name: 'Test server' });
    fireEvent.click(testServerBtn);

    await waitFor(() => {
      expect(screen.getByText(/Ready: 2 tool\(s\) discovered/i)).toBeTruthy();
    });
  });

  it('guards against accidental backdrop click when dirty', () => {
    const onClose = vi.fn();
    const { container } = render(<SettingsModal settings={baseSettings} onSave={vi.fn()} onClose={onClose} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));

    const tempInput = screen.getByDisplayValue('0.7');
    fireEvent.change(tempInput, { target: { value: '1.2' } });

    const backdrop = container.firstChild as HTMLElement;
    fireEvent.mouseDown(backdrop);

    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByText('You have unsaved changes. Click Save changes or Cancel.')).toBeTruthy();
  });

  it('treats MCP env draft edits as unsaved changes on backdrop click', () => {
    const onClose = vi.fn();
    const settingsWithMcp: AgentSettings = {
      ...baseSettings,
      mcpServers: [{ id: 'test-mcp', name: 'Mock MCP', command: 'node', args: [], env: {}, enabled: true }],
    };
    const { container } = render(<SettingsModal settings={settingsWithMcp} onSave={vi.fn()} onClose={onClose} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Skills & MCP' }));
    const envBox = screen.getByLabelText('Environment (KEY=value per line)');
    fireEvent.change(envBox, { target: { value: 'FOO=bar' } });
    fireEvent.mouseDown(container.firstChild as HTMLElement);
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByText('You have unsaved changes. Click Save changes or Cancel.')).toBeTruthy();
  });

  it('strips Windows CRLF trailing carriage returns when parsing MCP env', () => {
    const raw = 'API_KEY=sk-test1234\r\nENDPOINT=https://api.example.com\r\nEMPTY=\r\nSPACES=val with spaces\r\n';
    const parsed = parseEnv(raw);
    expect(parsed).toEqual({
      API_KEY: 'sk-test1234',
      ENDPOINT: 'https://api.example.com',
      EMPTY: '',
      SPACES: 'val with spaces',
    });
  });
});

describe('optional main-agent turn limit', () => {
  it('defaults missing limits to unlimited and saves zero without imposing a cap', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const { maxTurns: _previousLimit, ...withoutLimit } = baseSettings;
    const settings = withoutLimit as AgentSettings;
    render(<SettingsModal settings={settings} onSave={onSave} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));
    expect((screen.getByRole('checkbox', { name: 'Limit agent turns' }) as HTMLInputElement).checked).toBe(false);
    expect(screen.queryByRole('spinbutton', { name: 'Maximum agent turns' })).toBeNull();
    expect(screen.getByText(/runs until it finishes or you stop it/i)).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(onSave).toHaveBeenCalledOnce());
    expect(onSave.mock.calls[0][0].maxTurns).toBe(0);
  });

  it('preserves existing limits, supports more than 200, and lets users remove the cap', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<SettingsModal settings={baseSettings} onSave={onSave} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));
    const toggle = screen.getByRole('checkbox', { name: 'Limit agent turns' });
    expect((toggle as HTMLInputElement).checked).toBe(true);
    expect((screen.getByRole('spinbutton', { name: 'Maximum agent turns' }) as HTMLInputElement).value).toBe('24');
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Maximum agent turns' }), { target: { value: '500' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(onSave).toHaveBeenCalledOnce());
    expect(onSave.mock.calls[0][0].maxTurns).toBe(500);
    fireEvent.click(toggle);
    expect(screen.queryByRole('spinbutton', { name: 'Maximum agent turns' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
    expect(onSave.mock.calls[1][0].maxTurns).toBe(0);
    expect(baseSettings.maxTurns).toBe(24);
  });

  it.each(['', '0', '-1', '1.5', '4294967296'])('rejects invalid optional turn limit %s', async (value) => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<SettingsModal settings={baseSettings} onSave={onSave} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('tab', { name: 'Agent defaults' }));
    fireEvent.change(screen.getByRole('spinbutton', { name: 'Maximum agent turns' }), { target: { value } });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));
    expect(onSave).not.toHaveBeenCalled();
    expect(await screen.findByText(/maximum agent turns must be a positive whole number/i)).toBeTruthy();
  });
});
