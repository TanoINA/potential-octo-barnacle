import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { AgentSettings, SkillInfo } from '../types/agent';
import { SettingsModal } from './SettingsModal';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

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

beforeEach(() => { vi.mocked(invoke).mockReset(); });

describe('SettingsModal reflection opt-in', () => {
  it('saves an explicit boolean when opting in and back out without mutating the supplied settings', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const settings = structuredClone(baseSettings);
    render(<SettingsModal settings={settings} onSave={onSave} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Agent defaults' }));
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
    fireEvent.click(screen.getByRole('button', { name: 'Agent defaults' }));
    const toggle = screen.getByRole('checkbox', { name: 'Enable automatic self-improvement' }) as HTMLInputElement;
    expect(toggle.checked).toBe(true);
    fireEvent.click(toggle);
    expect(toggle.checked).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onSave).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledOnce();
    expect(settings.self_improvement_enabled).toBe(true);
  });

  it('normalizes a missing legacy preference to explicit false when saving from Providers', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    render(<SettingsModal settings={legacySettings()} onSave={onSave} onClose={onClose} />);
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    await waitFor(() => expect(onSave).toHaveBeenCalledOnce());
    expect(onSave.mock.calls[0][0].self_improvement_enabled).toBe(false);
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  });

  it('discloses the provider request, local writes, and on-demand commands even when disabled', () => {
    render(<SettingsModal settings={baseSettings} onSave={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Agent defaults' }));

    const toggle = screen.getByRole('checkbox', { name: 'Enable automatic self-improvement' });
    const descriptionId = toggle.getAttribute('aria-describedby');
    expect(descriptionId).toBeTruthy();
    const disclosure = document.getElementById(descriptionId!);
    expect(disclosure?.textContent).toContain('extra request to the active provider');
    expect(disclosure?.textContent).toContain('successful run with multiple turns or tool activity');
    expect(disclosure?.textContent).toContain('.aether/skills');
    expect(disclosure?.textContent).toContain('/review');
    expect(disclosure?.textContent).toContain('/reflect');
    expect(disclosure?.textContent).toContain('even when automatic self-improvement is disabled');
    expect(invoke).not.toHaveBeenCalled();
  });

  it('shows a disabled-by-default opt-in for legacy settings in Agent defaults', () => {
    render(<SettingsModal settings={legacySettings()} onSave={vi.fn()} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Agent defaults' }));

    expect(screen.getByRole('heading', { name: 'Agent Intelligence / Reflection' })).toBeTruthy();
    const toggle = screen.getByRole('checkbox', { name: 'Enable automatic self-improvement' });
    expect((toggle as HTMLInputElement).checked).toBe(false);
  });

  it('toggles and configures auto micro-compaction settings', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<SettingsModal settings={baseSettings} onSave={onSave} onClose={vi.fn()} />);
    fireEvent.click(screen.getByRole('button', { name: 'Agent defaults' }));

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
    fireEvent.click(screen.getByRole('button', { name: 'Skills & MCP' }));

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
