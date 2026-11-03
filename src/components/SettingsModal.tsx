import React, { useState } from 'react';
import { X, Plus, Trash2, Save, Key, Globe, Cpu, RefreshCw, Check, Sparkles } from 'lucide-react';
import type { AgentSettings, ProviderConfig } from '../types/agent';
import { invoke } from '@tauri-apps/api/core';

interface SettingsModalProps {
  settings: AgentSettings;
  onSave: (settings: AgentSettings) => void;
  onClose: () => void;
}

export const SettingsModal: React.FC<SettingsModalProps> = ({
  settings,
  onSave,
  onClose,
}) => {
  const [formData, setFormData] = useState<AgentSettings>({ ...settings });
  const [selectedProviderId, setSelectedProviderId] = useState<string>(settings.activeProviderId);

  // Model discovery state
  const [fetchedModels, setFetchedModels] = useState<string[]>([]);
  const [isFetchingModels, setIsFetchingModels] = useState(false);
  const [fetchStatus, setFetchStatus] = useState<string | null>(null);

  const selectedProvider = formData.providers.find((p) => p.id === selectedProviderId) || formData.providers[0];

  const handleProviderChange = (field: keyof ProviderConfig, value: string) => {
    setFormData((prev) => ({
      ...prev,
      providers: prev.providers.map((p) => (p.id === selectedProviderId ? { ...p, [field]: value } : p)),
    }));
  };

  const handleAddProvider = () => {
    const newId = `custom-${Date.now()}`;
    const newProv: ProviderConfig = {
      id: newId,
      name: 'Custom Provider',
      baseUrl: 'https://api.gutsai.id/v1',
      apiKey: '',
      model: 'kimi-k1.5-preview',
    };
    setFormData((prev) => ({
      ...prev,
      providers: [...prev.providers, newProv],
    }));
    setSelectedProviderId(newId);
    setFetchedModels([]);
    setFetchStatus(null);
  };

  const handleDeleteProvider = (id: string) => {
    if (formData.providers.length <= 1) {
      alert('Must have at least one provider.');
      return;
    }
    const remaining = formData.providers.filter((p) => p.id !== id);
    setFormData((prev) => ({
      ...prev,
      providers: remaining,
      activeProviderId: prev.activeProviderId === id ? remaining[0].id : prev.activeProviderId,
    }));
    if (selectedProviderId === id) {
      setSelectedProviderId(remaining[0].id);
    }
  };

  const handleFetchModels = async () => {
    if (!selectedProvider.baseUrl) {
      setFetchStatus('Please enter a Base URL first.');
      return;
    }

    setIsFetchingModels(true);
    setFetchStatus('Discovering models from provider...');

    try {
      const models = await invoke<string[]>('fetch_provider_models', {
        baseUrl: selectedProvider.baseUrl,
        apiKey: selectedProvider.apiKey,
      });

      setFetchedModels(models);
      if (models.length > 0) {
        setFetchStatus(`Found ${models.length} available models!`);
        // If current model not in list, auto-select the first one or smart pick
        if (!models.includes(selectedProvider.model)) {
          handleProviderChange('model', models[0]);
        }
      } else {
        setFetchStatus('Provider returned 0 models.');
      }
    } catch (err: any) {
      setFetchStatus(`Failed: ${err.message || String(err)}`);
    } finally {
      setIsFetchingModels(false);
    }
  };

  const handleSave = () => {
    onSave({
      ...formData,
      activeProviderId: selectedProviderId,
    });
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 bg-black/75 backdrop-blur-sm flex items-center justify-center p-4">
      <div className="w-full max-w-2xl bg-[#161b22] border border-[#30363d] rounded-2xl shadow-2xl flex flex-col overflow-hidden max-h-[90vh]">
        {/* Header */}
        <div className="p-4 border-b border-[#30363d] flex items-center justify-between bg-[#0d1117]">
          <div>
            <h2 className="text-sm font-semibold text-white flex items-center gap-2">
              <Sparkles className="w-4 h-4 text-cyan-400" />
              <span>Provider & Model Configuration</span>
            </h2>
            <p className="text-[11px] text-slate-400">Configure custom LLM endpoints (GutsAI, DeepSeek, Kimi, Ollama, OpenAI)</p>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 rounded-md text-slate-400 hover:text-white hover:bg-[#21262d] transition-colors"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        {/* Body */}
        <div className="flex-1 flex overflow-hidden">
          {/* Left: Providers List */}
          <div className="w-56 border-r border-[#30363d] bg-[#0d1117] p-3 flex flex-col">
            <div className="flex items-center justify-between mb-2">
              <span className="text-[11px] font-semibold tracking-wider text-slate-400 uppercase">Providers</span>
              <button
                onClick={handleAddProvider}
                className="p-1 rounded hover:bg-[#21262d] text-cyan-400 hover:text-cyan-300 transition-colors"
                title="Add Custom Provider"
              >
                <Plus className="w-3.5 h-3.5" />
              </button>
            </div>

            <div className="flex-1 overflow-y-auto space-y-1">
              {formData.providers.map((p) => {
                const isSelected = p.id === selectedProviderId;
                const isActive = p.id === formData.activeProviderId;
                return (
                  <div
                    key={p.id}
                    onClick={() => {
                      setSelectedProviderId(p.id);
                      setFetchedModels([]);
                      setFetchStatus(null);
                    }}
                    className={`flex items-center justify-between p-2 rounded-lg cursor-pointer text-xs transition-colors ${
                      isSelected
                        ? 'bg-[#1f6feb]/20 text-[#58a6ff] font-medium border border-[#388bfd]/30'
                        : 'text-slate-300 hover:bg-[#161b22]'
                    }`}
                  >
                    <div className="truncate pr-1">
                      <div className="truncate">{p.name}</div>
                      <div className="text-[10px] text-slate-400 font-mono truncate">{p.model}</div>
                    </div>

                    {isActive && (
                      <span className="w-2 h-2 rounded-full bg-emerald-400 shrink-0" title="Active Model" />
                    )}
                  </div>
                );
              })}
            </div>
          </div>

          {/* Right: Selected Provider Details */}
          <div className="flex-1 p-5 overflow-y-auto space-y-4">
            {selectedProvider && (
              <>
                <div>
                  <label className="block text-xs font-medium text-slate-300 mb-1">Provider Display Name</label>
                  <input
                    type="text"
                    value={selectedProvider.name}
                    onChange={(e) => handleProviderChange('name', e.target.value)}
                    className="w-full bg-[#0d1117] border border-[#30363d] rounded-lg px-3 py-2 text-xs text-white focus:outline-none focus:border-cyan-500 font-medium"
                  />
                </div>

                <div>
                  <label className="flex items-center gap-1.5 text-xs font-medium text-slate-300 mb-1">
                    <Globe className="w-3.5 h-3.5 text-cyan-400" />
                    <span>Base URL (OpenAI-compatible)</span>
                  </label>
                  <input
                    type="text"
                    value={selectedProvider.baseUrl}
                    onChange={(e) => handleProviderChange('baseUrl', e.target.value)}
                    placeholder="https://api.gutsai.id/v1"
                    className="w-full bg-[#0d1117] border border-[#30363d] rounded-lg px-3 py-2 text-xs text-white focus:outline-none focus:border-cyan-500 font-mono"
                  />
                  <span className="text-[10px] text-slate-400 mt-0.5 block">
                    Contoh: `https://api.gutsai.id/v1`, `https://api.deepseek.com/v1`, atau `http://localhost:11434/v1`
                  </span>
                </div>

                <div>
                  <label className="flex items-center gap-1.5 text-xs font-medium text-slate-300 mb-1">
                    <Key className="w-3.5 h-3.5 text-amber-400" />
                    <span>API Key</span>
                  </label>
                  <input
                    type="password"
                    value={selectedProvider.apiKey}
                    onChange={(e) => handleProviderChange('apiKey', e.target.value)}
                    placeholder="sk-... (kosongkan jika local Ollama)"
                    className="w-full bg-[#0d1117] border border-[#30363d] rounded-lg px-3 py-2 text-xs text-white focus:outline-none focus:border-cyan-500 font-mono"
                  />
                </div>

                {/* Auto Model Search Section */}
                <div className="pt-2 border-t border-[#30363d]/60">
                  <div className="flex items-center justify-between mb-1">
                    <label className="flex items-center gap-1.5 text-xs font-medium text-slate-300">
                      <Cpu className="w-3.5 h-3.5 text-purple-400" />
                      <span>Model Name</span>
                    </label>

                    <button
                      type="button"
                      onClick={handleFetchModels}
                      disabled={isFetchingModels}
                      className="flex items-center gap-1.5 px-2.5 py-1 rounded bg-[#21262d] hover:bg-[#30363d] text-cyan-400 text-[11px] font-medium transition-colors border border-[#30363d]"
                    >
                      <RefreshCw className={`w-3 h-3 ${isFetchingModels ? 'animate-spin' : ''}`} />
                      <span>{isFetchingModels ? 'Searching...' : 'Auto-Search Models'}</span>
                    </button>
                  </div>

                  <input
                    type="text"
                    list="models-datalist"
                    value={selectedProvider.model}
                    onChange={(e) => handleProviderChange('model', e.target.value)}
                    placeholder="Pilih atau ketik model (e.g. kimi-k1.5-preview)"
                    className="w-full bg-[#0d1117] border border-[#30363d] rounded-lg px-3 py-2 text-xs text-white focus:outline-none focus:border-cyan-500 font-mono"
                  />

                  {/* Datalist for autocomplete */}
                  <datalist id="models-datalist">
                    {fetchedModels.map((m) => (
                      <option key={m} value={m} />
                    ))}
                  </datalist>

                  {/* Status & Quick-Pick Tags */}
                  {fetchStatus && (
                    <div className="mt-1.5 text-[11px] text-cyan-400 flex items-center gap-1">
                      <span>{fetchStatus}</span>
                    </div>
                  )}

                  {fetchedModels.length > 0 && (
                    <div className="mt-2">
                      <span className="text-[10px] text-slate-400 uppercase font-semibold block mb-1">Available Models:</span>
                      <div className="flex flex-wrap gap-1 max-h-24 overflow-y-auto p-1 bg-[#0d1117] rounded-lg border border-[#30363d]">
                        {fetchedModels.map((m) => {
                          const isCur = selectedProvider.model === m;
                          return (
                            <button
                              key={m}
                              type="button"
                              onClick={() => handleProviderChange('model', m)}
                              className={`px-2 py-0.5 rounded text-[10px] font-mono transition-colors ${
                                isCur
                                  ? 'bg-cyan-500/20 text-cyan-300 font-semibold border border-cyan-500/40'
                                  : 'bg-[#161b22] text-slate-400 hover:text-white hover:bg-[#21262d]'
                              }`}
                            >
                              {m}
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  )}
                </div>

                <div className="pt-3 flex items-center justify-between border-t border-[#30363d]">
                  <button
                    onClick={() => handleDeleteProvider(selectedProvider.id)}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs text-rose-400 hover:text-white hover:bg-rose-600/30 transition-colors"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                    <span>Delete Provider</span>
                  </button>

                  <button
                    onClick={() => setFormData((prev) => ({ ...prev, activeProviderId: selectedProvider.id }))}
                    className={`flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${
                      formData.activeProviderId === selectedProvider.id
                        ? 'bg-emerald-600 text-white'
                        : 'bg-[#21262d] text-slate-300 hover:bg-[#30363d]'
                    }`}
                  >
                    {formData.activeProviderId === selectedProvider.id ? (
                      <>
                        <Check className="w-3.5 h-3.5" />
                        <span>Active Provider</span>
                      </>
                    ) : (
                      <span>Set as Active</span>
                    )}
                  </button>
                </div>
              </>
            )}
          </div>
        </div>

        {/* Footer */}
        <div className="p-3 border-t border-[#30363d] bg-[#0d1117] flex justify-end gap-2">
          <button
            onClick={onClose}
            className="px-4 py-2 rounded-lg text-xs text-slate-300 hover:bg-[#21262d] transition-colors"
          >
            Cancel
          </button>
          <button
            onClick={handleSave}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-cyan-600 hover:bg-cyan-500 text-white text-xs font-medium shadow-md shadow-cyan-600/20 transition-all"
          >
            <Save className="w-3.5 h-3.5" />
            <span>Save Changes</span>
          </button>
        </div>
      </div>
    </div>
  );
};
