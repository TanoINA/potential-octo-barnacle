import React, { useState } from 'react';
import { X, Plus, Trash2, Save, Key, Globe, Cpu } from 'lucide-react';
import type { AgentSettings, ProviderConfig } from '../types/agent';

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
      baseUrl: 'https://api.openai.com/v1',
      apiKey: '',
      model: 'gpt-4o',
    };
    setFormData((prev) => ({
      ...prev,
      providers: [...prev.providers, newProv],
    }));
    setSelectedProviderId(newId);
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

  const handleSave = () => {
    onSave({
      ...formData,
      activeProviderId: selectedProviderId,
    });
    onClose();
  };

  return (
    <div className="fixed inset-0 z-50 bg-black/70 backdrop-blur-sm flex items-center justify-center p-4">
      <div className="w-full max-w-2xl bg-[#161b22] border border-[#30363d] rounded-2xl shadow-2xl flex flex-col overflow-hidden max-h-[90vh]">
        {/* Header */}
        <div className="p-4 border-b border-[#30363d] flex items-center justify-between bg-[#0d1117]">
          <div>
            <h2 className="text-sm font-semibold text-white">Agent & Provider Settings</h2>
            <p className="text-[11px] text-slate-400">Configure custom LLM endpoints (GutsAI, DeepSeek, Kimi, Ollama, etc.)</p>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded-md text-slate-400 hover:text-white hover:bg-[#21262d] transition-colors"
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
                className="p-1 rounded hover:bg-[#21262d] text-cyan-400 hover:text-cyan-300"
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
                    onClick={() => setSelectedProviderId(p.id)}
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
                    placeholder="sk-..."
                    className="w-full bg-[#0d1117] border border-[#30363d] rounded-lg px-3 py-2 text-xs text-white focus:outline-none focus:border-cyan-500 font-mono"
                  />
                </div>

                <div>
                  <label className="flex items-center gap-1.5 text-xs font-medium text-slate-300 mb-1">
                    <Cpu className="w-3.5 h-3.5 text-purple-400" />
                    <span>Model Name</span>
                  </label>
                  <input
                    type="text"
                    value={selectedProvider.model}
                    onChange={(e) => handleProviderChange('model', e.target.value)}
                    placeholder="kimi-k1.5-preview / deepseek-chat"
                    className="w-full bg-[#0d1117] border border-[#30363d] rounded-lg px-3 py-2 text-xs text-white focus:outline-none focus:border-cyan-500 font-mono"
                  />
                </div>

                <div className="pt-2 flex items-center justify-between border-t border-[#30363d]">
                  <button
                    onClick={() => handleDeleteProvider(selectedProvider.id)}
                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs text-rose-400 hover:text-white hover:bg-rose-600/30 transition-colors"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                    <span>Delete Provider</span>
                  </button>

                  <button
                    onClick={() => setFormData((prev) => ({ ...prev, activeProviderId: selectedProvider.id }))}
                    className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-colors ${
                      formData.activeProviderId === selectedProvider.id
                        ? 'bg-emerald-600 text-white'
                        : 'bg-[#21262d] text-slate-300 hover:bg-[#30363d]'
                    }`}
                  >
                    {formData.activeProviderId === selectedProvider.id ? '✓ Active Provider' : 'Set as Active'}
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
            <span>Save Settings</span>
          </button>
        </div>
      </div>
    </div>
  );
};
