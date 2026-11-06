import { useEffect, useMemo, useRef, useState } from 'react';
import { Brain, Check, ChevronDown, Plus, RefreshCw, RotateCcw, Search, Star, X } from 'lucide-react';
import type {
  AgentSettings,
  EffectiveRunConfig,
  ReasoningEffort,
  SessionModelOverride,
} from '../types/agent';
import { useModalAccessibility } from '../hooks/useModalAccessibility';

const EFFORTS: { value: ReasoningEffort; label: string }[] = [
  { value: 'minimal', label: 'Minimal' },
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' },
  { value: 'extra_high', label: 'Extra High' },
  { value: 'max', label: 'Max' },
  { value: 'ultra', label: 'Ultra' },
];

interface ModelPickerProps {
  open: boolean;
  settings: AgentSettings;
  effective: EffectiveRunConfig;
  override?: SessionModelOverride | null;
  onClose: () => void;
  onSelect: (providerId: string, model: string) => void;
  onUpdateOverride: (override: Partial<SessionModelOverride>) => void;
  onClearOverride: () => void;
  onToggleFavorite: (key: string) => Promise<void>;
  onAddCustomModel: (providerId: string, model: string) => Promise<void>;
  onRefreshModels: (providerId: string) => Promise<string[]>;
}

interface ModelRow {
  providerId: string;
  providerName: string;
  model: string;
  key: string;
}

function badgesFor(model: string): string[] {
  const value = model.toLowerCase();
  const badges: string[] = [];
  if (/(^|[-_/.])max([-_/.]|$)/.test(value)) badges.push('MAX');
  if (/(^|[-_/.])high([-_/.]|$)/.test(value)) badges.push('HIGH');
  if (/preview|beta|experimental|[-_]exp([-_/.]|$)/.test(value)) badges.push('PREVIEW');
  return badges;
}

export function ModelPicker({
  open,
  settings,
  effective,
  override,
  onClose,
  onSelect,
  onUpdateOverride,
  onClearOverride,
  onToggleFavorite,
  onAddCustomModel,
  onRefreshModels,
}: ModelPickerProps) {
  const [search, setSearch] = useState('');
  const [providerFilter, setProviderFilter] = useState('all');
  const [customProviderId, setCustomProviderId] = useState(effective.provider.id);
  const [customMode, setCustomMode] = useState(false);
  const [thinkingOpen, setThinkingOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [pendingSelection, setPendingSelection] = useState<{ providerId: string; model: string }>({
    providerId: effective.provider.id,
    model: effective.provider.model,
  });
  const searchRef = useRef<HTMLInputElement>(null);
  const dialogRef = useModalAccessibility<HTMLElement>(open, () => {
    if (thinkingOpen) setThinkingOpen(false);
    else onClose();
  }, searchRef);

  useEffect(() => {
    if (open) {
      setSearch('');
      setCustomMode(false);
      setThinkingOpen(false);
      setStatus(null);
      setCustomProviderId(effective.provider.id);
      setPendingSelection({
        providerId: effective.provider.id,
        model: effective.provider.model,
      });
    }
  }, [open, effective.provider.id, effective.provider.model]);

  const rows = useMemo(() => {
    const query = (search || '').trim().toLowerCase();
    const all: ModelRow[] = [];
    for (const provider of settings.providers) {
      if (providerFilter !== 'all' && provider.id !== providerFilter) continue;
      const models = [...new Set([provider.model, ...(provider.models || []), ...(provider.customModels || [])].filter(Boolean))];
      for (const model of models) {
        if (query && !`${model} ${provider.name} ${provider.id}`.toLowerCase().includes(query)) continue;
        all.push({ providerId: provider.id, providerName: provider.name, model, key: `${provider.id}:${model}` });
      }
    }
    const favorites = new Set(settings.favoriteModels);
    return all.sort((a, b) => Number(favorites.has(b.key)) - Number(favorites.has(a.key)));
  }, [settings.providers, settings.favoriteModels, providerFilter, search]);

  if (!open) return null;

  const refresh = async () => {
    const providerId = providerFilter === 'all' ? effective.provider.id : providerFilter;
    setBusy(true);
    setStatus(null);
    try {
      const models = await onRefreshModels(providerId);
      setStatus(`Loaded ${models.length} model${models.length === 1 ? '' : 's'} from the provider.`);
    } catch (error) {
      setStatus(`Refresh failed: ${String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const addCustom = async () => {
    const model = (search || '').trim();
    if (!model) {
      setStatus('Enter a model ID in the search field first.');
      return;
    }
    setBusy(true);
    try {
      await onAddCustomModel(customProviderId, model);
      onClose();
    } catch (error) {
      setStatus(`Could not add model: ${String(error)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-black/70 px-4 pt-[8vh]"
      onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}
      role="presentation"
    >
      <section
        ref={dialogRef}
        tabIndex={-1}
        className="flex max-h-[84vh] w-full max-w-2xl flex-col overflow-hidden rounded-xl border border-[#30363d] bg-[#161b22] text-[#e6edf3] shadow-2xl"
        role="dialog"
        aria-modal="true"
        aria-label="Select model"
      >
        <header className="flex items-center justify-between border-b border-[#30363d] px-4 py-3">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold">Select model</h2>
            <p className="truncate text-xs text-[#8b949e]">{effective.provider.name} · {effective.provider.model} · {override ? 'Session override' : 'Settings default'}</p>
          </div>
          <button type="button" onClick={onClose} className="icon-button" aria-label="Close model picker"><X size={16} /></button>
        </header>

        <div className="border-b border-[#30363d] p-3">
          <div className="flex items-center gap-2 rounded-lg border border-[#30363d] bg-[#0d1117] px-3 focus-within:border-[#58a6ff]">
            <Search size={15} className="shrink-0 text-[#8b949e]" />
            <input
              ref={searchRef}
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={customMode ? 'Enter custom model ID' : 'Search models'}
              className="h-10 min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-[#6e7681]"
              aria-label="Search models"
            />
            {search && <button type="button" onClick={() => setSearch('')} className="icon-button" aria-label="Clear search"><X size={13} /></button>}
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <select
              value={providerFilter}
              onChange={(event) => setProviderFilter(event.target.value)}
              className="h-8 rounded-md border border-[#30363d] bg-[#0d1117] px-2 text-xs outline-none focus:border-[#58a6ff]"
              aria-label="Filter provider"
            >
              <option value="all">All providers</option>
              {settings.providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
            </select>
            <button type="button" className="toolbar-button" onClick={() => void refresh()} disabled={busy}>
              <RefreshCw size={13} className={busy ? 'spin' : ''} /> Refresh models
            </button>
            <div className="relative ml-auto">
              <button
                type="button"
                className={`toolbar-button ${effective.thinking ? 'border-[#58a6ff]/60 text-[#58a6ff]' : ''}`}
                onClick={() => setThinkingOpen((value) => !value)}
                aria-expanded={thinkingOpen}
              >
                <Brain size={13} /> Thinking <ChevronDown size={12} />
              </button>
              {thinkingOpen && (
                <div className="absolute right-0 z-10 mt-2 max-h-[calc(84vh-220px)] w-64 overflow-y-auto overscroll-contain rounded-lg border border-[#30363d] bg-[#161b22] p-3 shadow-2xl">
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-medium">Thinking</span>
                    <button
                      type="button"
                      aria-label="Enable thinking"
                      role="switch"
                      aria-checked={effective.thinking}
                      onClick={() => onUpdateOverride({ thinking: !effective.thinking })}
                      className={`relative h-5 w-9 rounded-full transition-colors ${effective.thinking ? 'bg-[#1f6feb]' : 'bg-[#30363d]'}`}
                    >
                      <span className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full bg-white transition-transform ${effective.thinking ? 'translate-x-4' : 'translate-x-0'}`} />
                    </button>
                  </div>
                  <fieldset className="mt-3 border-t border-[#30363d] pt-2">
                    <legend className="mb-1 text-[10px] font-semibold tracking-wider text-[#8b949e]">EFFORT</legend>
                    {EFFORTS.map((effort) => (
                      <label key={effort.value} className="flex cursor-pointer items-center gap-2 rounded px-1 py-1 text-xs hover:bg-[#21262d]">
                        <input
                          type="radio"
                          name="reasoning-effort"
                          value={effort.value}
                          checked={effective.reasoningEffort === effort.value}
                          onChange={() => onUpdateOverride({ reasoningEffort: effort.value })}
                          className="accent-[#58a6ff]"
                        />
                        {effort.label}
                      </label>
                    ))}
                  </fieldset>
                </div>
              )}
            </div>
          </div>
          {status && <p className="mt-2 text-xs text-[#8b949e]" role="status">{status}</p>}
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-2" role="listbox" aria-label="Available models">
          {rows.length === 0 ? (
            <div className="px-4 py-10 text-center text-sm text-[#8b949e]">No models match this search.</div>
          ) : rows.map((row) => {
            const isPending = row.providerId === pendingSelection.providerId && row.model === pendingSelection.model;
            const isCurrentActive = row.providerId === effective.provider.id && row.model === effective.provider.model;
            const favorite = settings.favoriteModels.includes(row.key);
            return (
              <div
                key={row.key}
                className={`group mb-0.5 flex items-center rounded-md border transition-colors ${
                  isPending
                    ? 'border-[#58a6ff] bg-[#1f6feb]/20'
                    : isCurrentActive
                    ? 'border-[#30363d] bg-[#161b22]'
                    : 'border-transparent hover:bg-[#21262d]'
                }`}
              >
                <button
                  type="button"
                  className="flex min-w-0 flex-1 items-center gap-2 px-3 py-2.5 text-left"
                  onClick={() => setPendingSelection({ providerId: row.providerId, model: row.model })}
                  onDoubleClick={() => {
                    onSelect(row.providerId, row.model);
                    onClose();
                  }}
                  role="option"
                  aria-selected={isPending}
                >
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-mono text-xs text-[#e6edf3]">{row.model}</span>
                    <span className="block truncate text-[10px] text-[#8b949e]">{row.providerName}</span>
                  </span>
                  {badgesFor(row.model).map((badge) => (
                    <span key={badge} className="rounded border border-[#30363d] bg-[#0d1117] px-1.5 py-0.5 text-[9px] font-semibold text-[#58a6ff]">{badge}</span>
                  ))}
                  {isCurrentActive && !isPending && (
                    <span className="rounded bg-[#21262d] px-1.5 py-0.5 text-[9px] font-medium text-[#8b949e]">CURRENT</span>
                  )}
                  {isPending && <Check size={14} className="shrink-0 text-[#58a6ff]" />}
                </button>
                <button
                  type="button"
                  onClick={() => void onToggleFavorite(row.key)}
                  className="mr-2 rounded p-1.5 text-[#8b949e] hover:bg-[#30363d] hover:text-[#e3b341]"
                  aria-label={favorite ? `Remove ${row.model} from favorites` : `Add ${row.model} to favorites`}
                >
                  <Star size={14} fill={favorite ? '#e3b341' : 'none'} className={favorite ? 'text-[#e3b341]' : ''} />
                </button>
              </div>
            );
          })}
        </div>

        <footer className="flex flex-wrap items-center justify-between gap-2 border-t border-[#30363d] bg-[#0d1117] p-3">
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="toolbar-button"
              onClick={() => {
                onClearOverride();
                setStatus('This chat now uses the Settings defaults.');
                const defaultProvider = settings.providers.find((p) => p.id === settings.activeProviderId) || settings.providers[0];
                if (defaultProvider) {
                  setPendingSelection({
                    providerId: defaultProvider.id,
                    model: defaultProvider.model,
                  });
                }
              }}
            >
              <RotateCcw size={13} /> Reset to default
            </button>
            <button
              type="button"
              className="toolbar-button"
              onClick={() => {
                setCustomMode((value) => !value);
                searchRef.current?.focus();
              }}
            >
              <Plus size={13} /> Custom model
            </button>
          </div>
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="max-w-full truncate text-[11px] text-[#8b949e]">
              Selected: <strong className="font-mono text-[#58a6ff]">{pendingSelection.model}</strong>
            </span>
            <button type="button" className="toolbar-button" onClick={onClose}>
              Cancel
            </button>
            <button
              type="button"
              className="primary-button"
              onClick={() => {
                onSelect(pendingSelection.providerId, pendingSelection.model);
                onClose();
              }}
            >
              <Check size={14} /> OK
            </button>
          </div>
        </footer>

        {customMode && (
          <div className="flex flex-wrap items-center gap-2 border-t border-[#30363d] bg-[#161b22] p-3">
            <select value={customProviderId} onChange={(event) => setCustomProviderId(event.target.value)} className="h-8 rounded border border-[#30363d] bg-[#0d1117] px-2 text-xs" aria-label="Custom model provider">
              {settings.providers.map((provider) => <option key={provider.id} value={provider.id}>{provider.name}</option>)}
            </select>
            <span className="min-w-0 flex-1 truncate font-mono text-xs text-[#8b949e]">{search || 'Model ID from search field'}</span>
            <button type="button" className="primary-button" onClick={() => void addCustom()} disabled={busy || !(search || '').trim()}>Add & use</button>
          </div>
        )}
      </section>
    </div>
  );
}
