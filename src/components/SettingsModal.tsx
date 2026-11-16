import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import {
  AlertCircle,
  Brain,
  CheckCircle2,
  Eye,
  EyeOff,
  FolderOpen,
  ListTree,
  Play,
  Plus,
  RefreshCw,
  RotateCcw,
  Save,
  Server,
  Trash2,
  Volume2,
  X,
} from 'lucide-react';
import { playTaskCompleteChime } from '../utils/audioChime';
import type { AgentSettings, ExecutionMode, McpServerConfig, ProviderConfig, ReasoningEffort, SkillInfo } from '../types/agent';
import { useModalAccessibility } from '../hooks/useModalAccessibility';

type SettingsTab = 'providers' | 'agent' | 'integrations';

interface SettingsModalProps {
  onAbout?: () => void;
  settings: AgentSettings;
  onSave: (settings: AgentSettings) => Promise<void>;
  onClose: () => void;
}

const EFFORTS: { value: ReasoningEffort; label: string }[] = [
  { value: 'minimal', label: 'Minimal' }, { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' }, { value: 'high', label: 'High' },
  { value: 'extra_high', label: 'Extra High' }, { value: 'max', label: 'Max' }, { value: 'ultra', label: 'Ultra' },
];

interface ProviderPreset {
  id: string;
  name: string;
  kind: ProviderConfig['kind'];
  baseUrl: string;
  model: string;
}

const PROVIDER_PRESETS: ProviderPreset[] = [
  { id: 'openai', name: 'OpenAI', kind: 'openai', baseUrl: 'https://api.openai.com/v1', model: 'gpt-4o' },
  { id: 'anthropic', name: 'Anthropic', kind: 'anthropic', baseUrl: 'https://api.anthropic.com', model: 'claude-3-7-sonnet-latest' },
  { id: 'deepseek', name: 'DeepSeek', kind: 'deepseek', baseUrl: 'https://api.deepseek.com/v1', model: 'deepseek-chat' },
  { id: 'openrouter', name: 'OpenRouter', kind: 'openai', baseUrl: 'https://openrouter.ai/api/v1', model: 'anthropic/claude-3.7-sonnet' },
  { id: 'groq', name: 'Groq', kind: 'openai', baseUrl: 'https://api.groq.com/openai/v1', model: 'llama-3.3-70b-versatile' },
  { id: 'ollama', name: 'Ollama (Local)', kind: 'ollama', baseUrl: 'http://localhost:11434/v1', model: 'llama3.2' },
];

function envText(server: McpServerConfig): string {
  return Object.entries(server.env).map(([key, value]) => `${key}=${value}`).join('\n');
}

export function parseEnv(value: string): Record<string, string> {
  const env: Record<string, string> = {};
  for (const line of value.split('\n')) {
    const separator = line.indexOf('=');
    if (separator > 0) env[line.slice(0, separator).trim()] = line.slice(separator + 1).replace(/\r$/, '');
  }
  return env;
}

export function SettingsModal({ settings, onSave, onClose, onAbout }: SettingsModalProps) {
  const [form, setForm] = useState<AgentSettings>(() => ({
    ...structuredClone(settings),
    self_improvement_enabled: settings.self_improvement_enabled ?? true,
    executionMode: settings.executionMode || 'turbo',
  }));
  const [envDrafts, setEnvDrafts] = useState<Record<string, string>>(() => Object.fromEntries(settings.mcpServers.map((server) => [server.id, envText(server)])));
  const [selectedProviderId, setSelectedProviderId] = useState(settings.activeProviderId || settings.providers[0]?.id || '');
  const [tab, setTab] = useState<SettingsTab>('providers');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [detectedSkills, setDetectedSkills] = useState<SkillInfo[]>([]);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [fetchStatus, setFetchStatus] = useState<string | null>(null);
  const [showCustomModel, setShowCustomModel] = useState(false);
  const [customModelDraft, setCustomModelDraft] = useState('');
  const [showApiKey, setShowApiKey] = useState(false);
  const [testingConnection, setTestingConnection] = useState(false);
  const [testConnectionResult, setTestConnectionResult] = useState<{ ok: boolean; message: string } | null>(null);
  const [testingMcpId, setTestingMcpId] = useState<string | null>(null);
  const [mcpTestResults, setMcpTestResults] = useState<Record<string, { ok: boolean; message: string }>>({});

  const dialogRef = useModalAccessibility<HTMLElement>(true, () => { if (!saving) onClose(); });
  const selectedProvider = form.providers.find((provider) => provider.id === selectedProviderId) || form.providers[0];

  const isDirty = useMemo(() => {
    const initialNormalized = {
      ...settings,
      self_improvement_enabled: settings.self_improvement_enabled ?? true,
      autoCompactionEnabled: settings.autoCompactionEnabled !== false,
      autoCompactionThresholdTokens: Number(settings.autoCompactionThresholdTokens) || 65536,
      executionMode: settings.executionMode || 'turbo',
    };
    const currentNormalized = {
      ...form,
      self_improvement_enabled: form.self_improvement_enabled === true,
      autoCompactionEnabled: form.autoCompactionEnabled !== false,
      autoCompactionThresholdTokens: Number(form.autoCompactionThresholdTokens) || 65536,
      executionMode: form.executionMode || 'turbo',
    };
    return JSON.stringify(initialNormalized) !== JSON.stringify(currentNormalized);
  }, [form, settings]);



  const allProviderModels = useMemo(() => {

    if (!selectedProvider) return [];

    return [

      ...new Set([

        selectedProvider.model,

        ...(selectedProvider.models || []),

        ...(selectedProvider.customModels || []),

      ].filter(Boolean)),

    ];

  }, [selectedProvider]);



  const handleRefreshModels = async (provider: ProviderConfig) => {
    setFetchingModels(true);
    setFetchStatus(null);
    try {
      const models = await invoke<string[]>('fetch_provider_models', { provider });
      updateProvider(provider.id, { models });
      setFetchStatus(`Successfully fetched ${models.length} models.`);
    } catch (err) {
      setFetchStatus(`Could not fetch models: ${String(err)}`);
    } finally {
      setFetchingModels(false);
    }
  };

  const handleTestConnection = async (provider: ProviderConfig) => {
    setTestingConnection(true);
    setTestConnectionResult(null);
    const start = performance.now();
    try {
      const models = await invoke<string[]>('fetch_provider_models', { provider });
      const elapsed = Math.round(performance.now() - start);
      setTestConnectionResult({
        ok: true,
        message: `Connection successful: ${models.length} model(s) discovered (${elapsed}ms)`,
      });
      if (models.length > 0) {
        updateProvider(provider.id, { models });
      }
    } catch (err) {
      setTestConnectionResult({
        ok: false,
        message: `Connection failed: ${String(err)}`,
      });
    } finally {
      setTestingConnection(false);
    }
  };

  const resetAgentDefaults = () => {
    setForm((current) => ({
      ...current,
      temperature: 0.7,
      maxTokens: 8192,
      maxTurns: 24,
      reasoningEffort: 'medium',
      thinking: false,
      streamResponse: true,
      notificationsEnabled: true,
      soundChimeEnabled: false,
      executionMode: 'turbo',
      self_improvement_enabled: true,
      autoCompactionEnabled: true,
      autoCompactionThresholdTokens: 65536,
      systemPrompt: '',
    }));
  };

  const handleBrowseSkillDirectory = async () => {
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected === 'string' && selected.trim()) {
        const clean = selected.trim();
        const existing = form.skillPaths.map((p) => p.trim()).filter(Boolean);
        if (!existing.includes(clean)) {
          setForm((current) => ({ ...current, skillPaths: [...existing, clean] }));
        }
      }
    } catch (err) {
      console.error('Failed to open folder picker:', err);
    }
  };

  const handleTestMcpServer = async (server: McpServerConfig) => {
    setTestingMcpId(server.id);
    try {
      const tools = await invoke<any[]>('list_mcp_tools', {
        server: {
          ...server,
          args: server.args.filter((argument) => argument.length > 0),
          env: parseEnv(envDrafts[server.id] ?? envText(server)),
        },
      });
      const count = Array.isArray(tools) ? tools.length : 0;
      setMcpTestResults((prev) => ({
        ...prev,
        [server.id]: {
          ok: true,
          message: `Ready: ${count} tool(s) discovered`,
        },
      }));
    } catch (err) {
      setMcpTestResults((prev) => ({
        ...prev,
        [server.id]: {
          ok: false,
          message: `Connection failed: ${String(err)}`,
        },
      }));
    } finally {
      setTestingMcpId(null);
    }
  };

  const applyCustomModel = () => {
    if (!selectedProvider || !(customModelDraft || '').trim()) return;
    const model = (customModelDraft || '').trim();
    updateProvider(selectedProvider.id, {
      model,
      customModels: [...new Set([...(selectedProvider.customModels || []), model])],
    });
    setCustomModelDraft('');
    setShowCustomModel(false);
  };

  useEffect(() => {
    if (tab !== 'integrations') return;
    let active = true;
    const paths = form.skillPaths.map((path) => (path || '').trim()).filter(Boolean);
    invoke<SkillInfo[]>('list_skills', { paths })
      .then((skills) => { if (active) setDetectedSkills(skills); })
      .catch((error) => { if (active) console.error(error); });
    return () => {
      active = false;
    };
  }, [tab, form.skillPaths]);

  const updateProvider = (id: string, patch: Partial<ProviderConfig>) => {
    setForm((current) => ({ ...current, providers: current.providers.map((provider) => provider.id === id ? { ...provider, ...patch } : provider) }));
  };

  const addProvider = () => {
    const id = `custom-${Date.now()}`;
    const provider: ProviderConfig = {
      id, name: 'Custom provider', kind: 'openai', baseUrl: 'https://api.openai.com/v1', apiKey: '', model: '',
      models: [], customModels: [], extraHeaders: {}, extraBody: {},
    };
    setForm((current) => ({ ...current, providers: [...current.providers, provider] }));
    setSelectedProviderId(id);
    setTestConnectionResult(null);
  };

  const deleteProvider = (id: string) => {
    if (form.providers.length <= 1) { setError('At least one provider is required.'); return; }
    const remaining = form.providers.filter((provider) => provider.id !== id);
    setForm((current) => ({
      ...current,
      providers: remaining,
      activeProviderId: current.activeProviderId === id ? remaining[0].id : current.activeProviderId,
    }));
    if (selectedProviderId === id) setSelectedProviderId(remaining[0].id);
  };

  const updateMcp = (id: string, patch: Partial<McpServerConfig>) => {
    setForm((current) => ({ ...current, mcpServers: current.mcpServers.map((server) => server.id === id ? { ...server, ...patch } : server) }));
  };

  const saveChanges = async () => {
    setSaving(true);
    setError(null);
    try {
      const normalized: AgentSettings = {
        ...form,
        self_improvement_enabled: form.self_improvement_enabled === true,
        autoCompactionEnabled: form.autoCompactionEnabled !== false,
        autoCompactionThresholdTokens: Number(form.autoCompactionThresholdTokens) || 65536,
        executionMode: form.executionMode || 'turbo',
        skillPaths: form.skillPaths.map((path) => (path || '').trim()).filter(Boolean),
        mcpServers: form.mcpServers.map((server) => ({
          ...server,
          args: server.args.filter((argument) => argument.length > 0),
          env: parseEnv(envDrafts[server.id] ?? envText(server)),
        })),
      };
      await onSave(normalized);
      onClose();
    } catch (reason) {
      setError(String(reason));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      className="ui-overlay-enter fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !saving) {
          if (isDirty) {
            setError('You have unsaved changes. Click Save changes or Cancel.');
            return;
          }
          onClose();
        }
      }}
    >

      <section ref={dialogRef} tabIndex={-1} className="ui-panel-enter flex max-h-[92vh] w-full max-w-4xl flex-col overflow-hidden rounded-xl border border-[#30363d] bg-[#161b22] text-[#e6edf3] shadow-lg" role="dialog" aria-modal="true" aria-label="AetherAgent settings">

        <header className="flex items-center justify-between border-b border-[#30363d] bg-[#0d1117] px-4 py-3">

          <div><h2 className="text-sm font-semibold">Settings</h2><p className="text-[11px] text-[#8b949e]">Providers, agent defaults, skills, and MCP</p></div>

          {onAbout && <button type="button" className="toolbar-button" onClick={onAbout} disabled={saving}>About AetherAgent</button>}
          <button type="button" className="icon-button" onClick={onClose} disabled={saving} aria-label="Close settings"><X size={16} /></button>

        </header>

        <div className="flex shrink-0 flex-wrap gap-1 border-b border-[#30363d] px-3 pt-2" role="tablist" aria-label="Settings sections">

          {([

            ['providers', 'Providers', Server], ['agent', 'Agent defaults', Brain], ['integrations', 'Skills & MCP', ListTree],

          ] as const).map(([id, label, Icon]) => (

            <button key={id} type="button" role="tab" id={`settings-tab-${id}`} aria-selected={tab === id} aria-controls="settings-tabpanel" tabIndex={tab === id ? 0 : -1} onClick={() => setTab(id)} onKeyDown={(event) => {
              if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
              event.preventDefault();
              const tabs = ['providers', 'agent', 'integrations'] as const;
              const index = tabs.indexOf(tab);
              const nextIndex = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : (index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
              setTab(tabs[nextIndex]);
              requestAnimationFrame(() => document.getElementById(`settings-tab-${tabs[nextIndex]}`)?.focus());
            }} className={`flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs ${tab === id ? 'border-[#58a6ff] text-[#58a6ff]' : 'border-transparent text-[#8b949e]'}`}><Icon size={13} />{label}</button>

          ))}

        </div>



        <div id="settings-tabpanel" role="tabpanel" aria-labelledby={`settings-tab-${tab}`} className="min-h-0 flex-1 overflow-y-auto">

          {tab === 'providers' && (

            <div className="flex min-h-[430px] flex-col sm:flex-row">

              <div className="w-full shrink-0 border-b border-[#30363d] bg-[#0d1117] p-3 sm:w-56 sm:border-b-0 sm:border-r">

                <div className="mb-2 flex items-center justify-between"><span className="text-[10px] font-semibold uppercase tracking-wider text-[#8b949e]">Providers</span><button type="button" className="icon-button" onClick={addProvider} aria-label="Add provider"><Plus size={14} /></button></div>

                {form.providers.map((provider) => (

                  <button key={provider.id} type="button" onClick={() => setSelectedProviderId(provider.id)} className={`mb-1 flex w-full items-center rounded-md border px-2.5 py-2 text-left ${provider.id === selectedProvider?.id ? 'border-[#58a6ff]/40 bg-[#1f6feb]/15' : 'border-transparent hover:bg-[#161b22]'}`}>

                    <span className="min-w-0 flex-1"><span className="block truncate text-xs">{provider.name}</span><span className="block truncate font-mono text-[9px] text-[#6e7681]">{provider.model || 'No model'}</span></span>

                    {provider.id === form.activeProviderId && <span className="h-2 w-2 rounded-full bg-[#3fb950]" title="Settings default provider" />}

                  </button>

                ))}

              </div>

              {selectedProvider && (
                <div className="min-w-0 flex-1 space-y-4 p-5">
                  <div className="flex items-center justify-between border-b border-[#30363d]/60 pb-3">
                    <div>
                      <span className="text-xs font-semibold">Provider configuration</span>
                      <p className="text-[10px] text-[#8b949e]">Configure API endpoint and model parameters</p>
                    </div>
                    <select
                      className="rounded border border-[#30363d] bg-[#0d1117] px-2 py-1 text-xs text-[#e6edf3] outline-none"
                      value=""
                      onChange={(e) => {
                        const preset = PROVIDER_PRESETS.find((p) => p.id === e.target.value);
                        if (preset && selectedProvider) {
                          updateProvider(selectedProvider.id, {
                            name: selectedProvider.name === 'Custom provider' || !selectedProvider.name ? preset.name : selectedProvider.name,
                            kind: preset.kind,
                            baseUrl: preset.baseUrl,
                            model: preset.model,
                          });
                          setTestConnectionResult(null);
                        }
                      }}
                      aria-label="Load provider preset"
                    >
                      <option value="" disabled>Load from preset...</option>
                      {PROVIDER_PRESETS.map((p) => (
                        <option key={p.id} value={p.id}>{p.name}</option>
                      ))}
                    </select>
                  </div>

                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                    <label className="settings-label">Display name<input className="settings-input" value={selectedProvider.name} onChange={(event) => updateProvider(selectedProvider.id, { name: event.target.value })} /></label>
                    <label className="settings-label">Protocol<select className="settings-input" value={selectedProvider.kind} onChange={(event) => updateProvider(selectedProvider.id, { kind: event.target.value as ProviderConfig['kind'] })}><option value="openai">OpenAI-compatible</option><option value="anthropic">Anthropic</option><option value="deepseek">DeepSeek</option><option value="ollama">Ollama local</option></select></label>
                  </div>

                  <label className="settings-label">Base URL<input className="settings-input font-mono" value={selectedProvider.baseUrl} onChange={(event) => updateProvider(selectedProvider.id, { baseUrl: event.target.value })} placeholder="https://api.openai.com/v1" /></label>

                  <div className="space-y-1">
                    <div className="flex items-center justify-between">
                      <label className="settings-label mb-0">API key</label>
                      <button
                        type="button"
                        className="flex items-center gap-1 text-[11px] text-[#8b949e] hover:text-[#e6edf3]"
                        onClick={() => setShowApiKey((v) => !v)}
                        title={showApiKey ? 'Hide API key' : 'Show API key'}
                        aria-label={showApiKey ? 'Hide API key' : 'Show API key'}
                      >
                        {showApiKey ? <EyeOff size={12} /> : <Eye size={12} />}
                        {showApiKey ? 'Hide' : 'Show'}
                      </button>
                    </div>
                    <input
                      type={showApiKey ? 'text' : 'password'}
                      autoComplete="off"
                      className="settings-input font-mono"
                      value={selectedProvider.apiKey}
                      onChange={(event) => {
                        updateProvider(selectedProvider.id, { apiKey: event.target.value });
                        setTestConnectionResult(null);
                      }}
                      placeholder="Leave empty for local providers"
                    />
                  </div>

                  <div className="flex items-center justify-between">
                    <button
                      type="button"
                      className="toolbar-button text-xs flex items-center gap-1.5"
                      onClick={() => void handleTestConnection(selectedProvider)}
                      disabled={testingConnection}
                      title="Test connection to provider endpoint and fetch model list"
                    >
                      <RefreshCw size={11} className={testingConnection ? 'spin' : ''} />
                      {testingConnection ? 'Testing connection…' : 'Test connection'}
                    </button>
                  </div>

                  {testConnectionResult && (
                    <div
                      className={`flex items-center gap-2 rounded px-2.5 py-1.5 text-xs ${
                        testConnectionResult.ok
                          ? 'border border-[#238636]/40 bg-[#238636]/15 text-[#3fb950]'
                          : 'border border-[#da3633]/40 bg-[#da3633]/15 text-[#f85149]'
                      }`}
                    >
                      {testConnectionResult.ok ? <CheckCircle2 size={13} className="shrink-0" /> : <AlertCircle size={13} className="shrink-0" />}
                      <span className="truncate">{testConnectionResult.message}</span>
                    </div>
                  )}

                  <div className="space-y-1.5">

                    <div className="flex items-center justify-between">

                      <label className="settings-label mb-0">Default model</label>

                      <button

                        type="button"

                        className="flex items-center gap-1 text-[11px] text-[#58a6ff] hover:underline"

                        onClick={() => void handleRefreshModels(selectedProvider)}

                        disabled={fetchingModels}

                        title="Fetch available models from provider API"

                      >

                        <RefreshCw size={11} className={fetchingModels ? 'spin' : ''} />

                        {fetchingModels ? 'Fetching…' : 'Refresh models from API'}

                      </button>

                    </div>



                    {allProviderModels.length > 0 ? (

                      <div className="space-y-2">

                        <div className="flex gap-2">

                          <select

                            className="settings-input font-mono flex-1 text-xs"

                            value={selectedProvider.model}

                            onChange={(event) => updateProvider(selectedProvider.id, { model: event.target.value })}

                            aria-label="Default model selector"

                          >

                            {!allProviderModels.includes(selectedProvider.model) && selectedProvider.model && (

                              <option value={selectedProvider.model}>{selectedProvider.model} (current)</option>

                            )}

                            {allProviderModels.map((m) => (

                              <option key={m} value={m}>{m}</option>

                            ))}

                          </select>

                          <button

                            type="button"

                            onClick={() => setShowCustomModel((value) => !value)}

                            className={`toolbar-button text-xs ${showCustomModel ? 'border-[#58a6ff] text-[#58a6ff]' : ''}`}

                            title="Type custom model ID manually"

                          >

                            Custom ID

                          </button>

                        </div>



                        {showCustomModel && (

                          <div className="flex gap-2">

                            <input

                              className="settings-input font-mono flex-1 text-xs"

                              placeholder="Type custom model ID..."

                              value={customModelDraft}

                              onChange={(event) => setCustomModelDraft(event.target.value)}

                              onKeyDown={(event) => {

                                if (event.key === 'Enter') {

                                  event.preventDefault();

                                  applyCustomModel();

                                }

                              }}

                            />

                            <button

                              type="button"

                              className="primary-button text-xs shrink-0"

                              disabled={!(customModelDraft || '').trim()}

                              onClick={applyCustomModel}

                            >

                              Apply

                            </button>

                          </div>

                        )}

                      </div>

                    ) : (

                      <div className="flex gap-2">

                        <input

                          className="settings-input font-mono flex-1 text-xs"

                          value={selectedProvider.model}

                          onChange={(event) => updateProvider(selectedProvider.id, { model: event.target.value })}

                          placeholder="Model ID (e.g. gpt-4o, claude-3-7-sonnet)"

                        />

                        <button

                          type="button"

                          className="toolbar-button text-xs shrink-0"

                          onClick={() => void handleRefreshModels(selectedProvider)}

                          disabled={fetchingModels}

                        >

                          Fetch list

                        </button>

                      </div>

                    )}

                    {fetchStatus && <p className="text-[10px] text-[#8b949e]">{fetchStatus}</p>}

                  </div>

                  <div className="flex items-center justify-between border-t border-[#30363d] pt-3">

                    <button type="button" className="danger-button" onClick={() => deleteProvider(selectedProvider.id)}><Trash2 size={13} /> Delete provider</button>

                    <button type="button" className="toolbar-button" onClick={() => setForm((current) => ({ ...current, activeProviderId: selectedProvider.id }))}>{form.activeProviderId === selectedProvider.id ? 'Settings default' : 'Use as Settings default'}</button>

                  </div>

                </div>

              )}

            </div>

          )}



          {tab === 'agent' && (
            <div className="mx-auto max-w-2xl space-y-4 p-5">
              <div className="flex items-center justify-between border-b border-[#30363d]/60 pb-2">
                <div>
                  <span className="text-xs font-semibold">Agent Behavior & Limits</span>
                  <p className="text-[10px] text-[#8b949e]">Configure execution autonomy and reasoning parameters</p>
                </div>
                <button
                  type="button"
                  className="toolbar-button flex items-center gap-1.5 text-xs text-[#8b949e] hover:text-[#e6edf3]"
                  onClick={resetAgentDefaults}
                  title="Reset all agent defaults to recommended factory values"
                >
                  <RotateCcw size={11} />
                  Reset to defaults
                </button>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <label className="settings-label">Temperature<input type="number" min="0" max="2" step="0.1" className="settings-input" value={form.temperature} onChange={(event) => setForm({ ...form, temperature: Number(event.target.value) })} /></label>
                <label className="settings-label">Max tokens<input type="number" min="1" step="1" className="settings-input" value={form.maxTokens ?? ''} onChange={(event) => setForm({ ...form, maxTokens: event.target.value ? Number(event.target.value) : undefined })} placeholder="Provider default" /></label>
                <label className="settings-label">Maximum agent turns<input type="number" min="1" max="200" className="settings-input" value={form.maxTurns} onChange={(event) => setForm({ ...form, maxTurns: Number(event.target.value) })} /></label>
                <label className="settings-label">Default reasoning effort<select className="settings-input" value={form.reasoningEffort} onChange={(event) => setForm({ ...form, reasoningEffort: event.target.value as ReasoningEffort })}>{EFFORTS.map((effort) => <option key={effort.value} value={effort.value}>{effort.label}</option>)}</select></label>
                <label className="settings-label sm:col-span-2">
                  Default execution mode
                  <select
                    className="settings-input"
                    value={form.executionMode ?? 'turbo'}
                    onChange={(event) => setForm({ ...form, executionMode: event.target.value as ExecutionMode })}
                    aria-label="Default execution mode"
                  >
                    <option value="turbo">Turbo (full autonomous execution without extra confirmation)</option>
                    <option value="auto">Auto (standard autonomous tool use)</option>
                    <option value="default">Default (conservative tool execution)</option>
                    <option value="request_review">Request Review (confirm sensitive tool calls)</option>
                  </select>
                </label>
              </div>

              <div className="flex flex-wrap gap-5 border-y border-[#30363d]/60 py-3">

                <label className="flex items-center gap-2 text-xs"><input type="checkbox" className="accent-[#58a6ff]" checked={form.thinking} onChange={(event) => setForm({ ...form, thinking: event.target.checked })} /> Enable thinking by default</label>

                <label className="flex items-center gap-2 text-xs"><input type="checkbox" className="accent-[#58a6ff]" checked={form.streamResponse} onChange={(event) => setForm({ ...form, streamResponse: event.target.checked })} /> Stream responses</label>

                <label className="flex items-center gap-2 text-xs" title="Show desktop notifications when AetherAgent is in background"><input type="checkbox" className="accent-[#58a6ff]" checked={form.notificationsEnabled ?? true} onChange={(event) => setForm({ ...form, notificationsEnabled: event.target.checked })} /> Desktop notifications</label>

                <div className="flex items-center gap-1.5">

                  <label className="flex items-center gap-2 text-xs" title="Audio chime synthesizer on finish (Default: OFF / muted)"><input type="checkbox" className="accent-[#58a6ff]" checked={form.soundChimeEnabled ?? false} onChange={(event) => setForm({ ...form, soundChimeEnabled: event.target.checked })} /> Sound chime on task finish <span className="text-[10px] text-[#8b949e]">(Default: Off)</span></label>

                  <button type="button" onClick={() => void playTaskCompleteChime()} className="flex items-center gap-1 rounded border border-[#30363d] bg-[#161b22] px-1.5 py-0.5 text-[10px] text-[#58a6ff] hover:bg-[#21262d]" title="Test synthesized audio chime"><Volume2 size={10} /> Test</button>

                </div>

              </div>

              <section className="space-y-2" aria-labelledby="reflection-settings-heading">

                <h3 id="reflection-settings-heading" className="text-xs font-semibold">Agent Intelligence / Reflection</h3>

                <label className="flex items-center gap-2 text-xs">

                  <input type="checkbox" className="accent-[#58a6ff]" checked={form.self_improvement_enabled ?? true} aria-describedby="reflection-settings-description" onChange={(event) => setForm({ ...form, self_improvement_enabled: event.target.checked })} />

                  Enable automatic self-improvement

                </label>

                <div className="grid gap-3 sm:grid-cols-2">
                  <label className="space-y-1 text-xs">Creation nudge interval
                    <input aria-label="Creation nudge interval" type="number" min={0} step={1} className="settings-input" value={form.reviewConfig?.creationNudgeInterval ?? 10} onChange={(event) => setForm({ ...form, reviewConfig: { ...form.reviewConfig, creationNudgeInterval: Math.max(0, Math.floor(Number(event.target.value) || 0)) } })} />
                    <span className="block text-[11px] text-[#8b949e]">Provider iterations across turns between reviews. 0 disables automatic reviews.</span>
                  </label>
                  <label className="flex items-center gap-2 text-xs">
                    <input type="checkbox" className="accent-[#58a6ff]" checked={form.reviewConfig?.writeApproval ?? false} onChange={(event) => setForm({ ...form, reviewConfig: { ...form.reviewConfig, writeApproval: event.target.checked } })} />
                    Require approval before writing skills
                  </label>
                  <label className="space-y-1 text-xs">Review provider
                    <select className="settings-input" value={form.reviewConfig?.auxiliaryProviderId ?? ''} onChange={(event) => setForm({ ...form, reviewConfig: { ...form.reviewConfig, auxiliaryProviderId: event.target.value || undefined } })}>
                      <option value="">Use current provider</option>
                      {form.providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
                    </select>
                  </label>
                  <label className="space-y-1 text-xs">Skill creation directory
                    <input type="text" className="settings-input" placeholder="Default global skills directory" value={form.reviewConfig?.createDir ?? ''} onChange={(event) => setForm({ ...form, reviewConfig: { ...form.reviewConfig, createDir: event.target.value || undefined } })} />
                  </label>
                </div>

                <p id="reflection-settings-description" className="text-[11px] leading-5 text-[#8b949e]">

                  Automatic reflection uses a creation nudge interval (default 10 provider iterations across turns; 0 disables automatic reviews). It makes an extra request to the active provider or selected review provider and can continue for up to 16 review iterations and may create or update SKILL.md files in the global skills directory or your chosen directory. Enable write approval to stage proposals before any skill writes. No workspace is required. The /review, /reflect and /refine commands work on demand even when automatic self-improvement is disabled.

                </p>

              </section>

              <section className="space-y-2 border-t border-[#30363d]/60 pt-3" aria-labelledby="compaction-settings-heading">

                <div className="flex items-center justify-between">

                  <h3 id="compaction-settings-heading" className="text-xs font-semibold">Auto Micro-Compaction</h3>

                  <select

                    value={form.autoCompactionThresholdTokens ?? 65536}

                    disabled={form.autoCompactionEnabled === false}

                    onChange={(e) => setForm({ ...form, autoCompactionThresholdTokens: Number(e.target.value) })}

                    className="rounded border border-[#30363d] bg-[#0d1117] px-2 py-1 text-xs text-[#e6edf3] outline-none"

                    aria-label="Micro-compaction token threshold"

                  >

                    <option value={32768}>32K tokens</option>

                    <option value={65536}>64K tokens (Default)</option>

                    <option value={98304}>96K tokens</option>

                    <option value={131072}>128K tokens</option>

                  </select>

                </div>

                <label className="flex items-center gap-2 text-xs">

                  <input

                    type="checkbox"

                    className="accent-[#58a6ff]"

                    checked={form.autoCompactionEnabled ?? true}

                    aria-describedby="compaction-settings-description"

                    onChange={(event) => setForm({ ...form, autoCompactionEnabled: event.target.checked })}

                  />

                  Enable automatic micro-compaction of verbose tool outputs

                </label>

                <p id="compaction-settings-description" className="text-[11px] leading-5 text-[#8b949e]">

                  Checks each model request and replaces verbose older tool outputs with receipts when the estimated context is large. Keeps the newest tool batch intact; full outputs stay in chat history. This is an estimate, not a model context-limit guarantee.

                </p>

              </section>

              <label className="settings-label">Custom system prompt<textarea className="settings-input min-h-48 resize-y font-mono leading-5" value={form.systemPrompt} onChange={(event) => setForm({ ...form, systemPrompt: event.target.value })} placeholder="Additional operator instructions appended to the native agent prompt" /></label>

            </div>

          )}



          {tab === 'integrations' && (

            <div className="space-y-5 p-5">

              <section>
                <div className="mb-2 flex items-center justify-between">
                  <div>
                    <h3 className="text-xs font-semibold">Skill roots</h3>
                    <p className="mt-0.5 text-[11px] leading-5 text-[#8b949e]">The loader checks the official ~/.aether-agent/skills directory, workspace skills, and system skill directories. Add custom directories per line.</p>
                  </div>
                  <button
                    type="button"
                    className="toolbar-button text-xs flex items-center gap-1.5 shrink-0"
                    onClick={() => void handleBrowseSkillDirectory()}
                    title="Choose skill directory using folder picker"
                  >
                    <FolderOpen size={12} />
                    Browse folder
                  </button>
                </div>

                <textarea className="settings-input min-h-20 resize-y font-mono" value={form.skillPaths.join('\n')} onChange={(event) => setForm({ ...form, skillPaths: event.target.value.split('\n') })} placeholder="D:\path\to\skills" />



                <div className="mt-3">

                  <div className="mb-1.5 flex items-center justify-between">

                    <span className="text-[11px] font-semibold text-[#e6edf3]">

                      Active Detected Skills ({detectedSkills.length})

                    </span>

                  </div>

                  <div className="max-h-40 space-y-1 overflow-y-auto rounded-lg border border-[#30363d] bg-[#0d1117] p-2">

                    {detectedSkills.length === 0 ? (

                      <div className="py-2 text-center text-xs text-[#6e7681]">No skills detected</div>

                    ) : (

                      detectedSkills.map((sk) => (

                        <div key={sk.path} className="flex items-start justify-between gap-2 rounded px-2 py-1.5 hover:bg-[#161b22]">

                          <div className="min-w-0">

                            <span className="block font-mono text-xs font-medium text-[#58a6ff]">{sk.name}</span>

                            <span className="line-clamp-1 text-[10px] text-[#8b949e]">{sk.description || 'No description'}</span>

                          </div>

                          <span className="shrink-0 rounded bg-[#21262d] px-1.5 py-0.5 text-[9px] uppercase tracking-wider text-[#8b949e]">

                            {sk.source}

                          </span>

                        </div>

                      ))

                    )}

                  </div>

                </div>

              </section>



              <section>

                <div className="mb-2 flex items-center justify-between"><div><h3 className="text-xs font-semibold">MCP stdio servers</h3><p className="mt-1 text-[11px] text-[#8b949e]">Enabled servers are initialized with JSON-RPC and their tools join the agent catalog.</p></div><button type="button" className="toolbar-button" onClick={() => setForm({ ...form, mcpServers: [...form.mcpServers, { id: `mcp-${Date.now()}`, name: 'MCP server', command: '', args: [], env: {}, enabled: false }] })}><Plus size={13} /> Add server</button></div>

                {form.mcpServers.length === 0 && <div className="rounded-lg border border-dashed border-[#30363d] px-4 py-6 text-center text-xs text-[#6e7681]">No MCP servers configured</div>}

                {form.mcpServers.map((server) => (

                  <div key={server.id} className="mb-3 space-y-3 rounded-lg border border-[#30363d] bg-[#0d1117] p-3">

                    <div className="flex items-center gap-2">
                      <input type="checkbox" checked={server.enabled} onChange={(event) => updateMcp(server.id, { enabled: event.target.checked })} className="accent-[#58a6ff]" aria-label={`Enable ${server.name}`} />
                      <input className="settings-input flex-1" value={server.name} onChange={(event) => updateMcp(server.id, { name: event.target.value })} aria-label="MCP server name" />
                      <button
                        type="button"
                        className="toolbar-button text-xs flex items-center gap-1 shrink-0"
                        onClick={() => void handleTestMcpServer(server)}
                        disabled={testingMcpId === server.id}
                        title="Test MCP server connection and tool discovery"
                      >
                        <Play size={10} className={testingMcpId === server.id ? 'spin' : ''} />
                        {testingMcpId === server.id ? 'Testing…' : 'Test server'}
                      </button>
                      <button type="button" className="icon-button text-[#f85149]" onClick={() => setForm({ ...form, mcpServers: form.mcpServers.filter((item) => item.id !== server.id) })} aria-label="Delete MCP server"><Trash2 size={14} /></button>
                    </div>

                    <label className="settings-label">Command<input className="settings-input font-mono" value={server.command} onChange={(event) => updateMcp(server.id, { command: event.target.value })} placeholder="node or absolute executable path" /></label>

                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                      <label className="settings-label">Arguments (one per line)<textarea className="settings-input min-h-20 resize-y font-mono" value={server.args.join('\n')} onChange={(event) => updateMcp(server.id, { args: event.target.value.split('\n') })} /></label>
                      <label className="settings-label">Environment (KEY=value per line)<textarea className="settings-input min-h-20 resize-y font-mono" value={envDrafts[server.id] ?? envText(server)} onChange={(event) => setEnvDrafts((current) => ({ ...current, [server.id]: event.target.value }))} spellCheck={false} /></label>
                    </div>

                    {mcpTestResults[server.id] && (
                      <div
                        className={`flex items-center gap-2 rounded px-2.5 py-1.5 text-xs ${
                          mcpTestResults[server.id].ok
                            ? 'border border-[#238636]/40 bg-[#238636]/15 text-[#3fb950]'
                            : 'border border-[#da3633]/40 bg-[#da3633]/15 text-[#f85149]'
                        }`}
                      >
                        {mcpTestResults[server.id].ok ? <CheckCircle2 size={12} className="shrink-0" /> : <AlertCircle size={12} className="shrink-0" />}
                        <span className="truncate">{mcpTestResults[server.id].message}</span>
                      </div>
                    )}

                  </div>

                ))}

              </section>



            </div>

          )}

        </div>



        <footer className="flex shrink-0 items-center gap-2 border-t border-[#30363d] bg-[#0d1117] p-3">

          {error && <span className="min-w-0 flex-1 truncate text-xs text-[#f85149]" title={error}>{error}</span>}

          <div className="ml-auto flex gap-2"><button type="button" className="toolbar-button" onClick={onClose} disabled={saving}>Cancel</button><button type="button" className="primary-button" onClick={() => void saveChanges()} disabled={saving || form.providers.length === 0}><Save size={13} /> {saving ? 'Saving…' : 'Save changes'}</button></div>

        </footer>

      </section>

    </div>

  );

}
