import React, { useEffect, useRef, useState } from 'react';
import { ArrowRight, CornerDownLeft, FileSearch, GitBranch, Terminal } from 'lucide-react';
import { AetherLogo } from './AetherLogo';
import { useModalAccessibility } from '../hooks/useModalAccessibility';

interface SpotlightBarProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmitPrompt: (prompt: string) => Promise<boolean>;
  onRunCommand?: (command: string) => void;
}

const QUICK_ACTIONS = [
  { id: 'explain', label: 'Explain codebase architecture', icon: FileSearch, prompt: 'Please give an executive overview of this codebase architecture and its primary entry points.' },
  { id: 'test', label: 'Run project tests & verify health', icon: Terminal, prompt: 'Please run the test suite for this workspace and verify if all unit and integration tests pass.' },
  { id: 'git_status', label: 'Check Git diff & commit status', icon: GitBranch, prompt: 'Please check git status and summarize the latest changes made to this workspace.' },
];

export const SpotlightBar: React.FC<SpotlightBarProps> = ({
  isOpen,
  onClose,
  onSubmitPrompt,
}) => {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const dialogRef = useModalAccessibility<HTMLDivElement>(isOpen, onClose, inputRef);

  useEffect(() => {
    if (isOpen) {
      setQuery('');
      setSelectedIndex(0);
    }
  }, [isOpen]);

  if (!isOpen) return null;

  const handleSubmit = async (customPrompt?: string) => {
    const text = customPrompt || (query || '').trim();
    if (!text) return;
    const accepted = await onSubmitPrompt(text);
    if (accepted) onClose();
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      if (e.nativeEvent.isComposing) return;
      e.preventDefault();
      if ((query || '').trim()) {
        void handleSubmit(query);
      } else if (QUICK_ACTIONS[selectedIndex]) {
        void handleSubmit(QUICK_ACTIONS[selectedIndex].prompt);
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSelectedIndex((prev) => (prev + 1) % QUICK_ACTIONS.length);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelectedIndex((prev) => (prev - 1 + QUICK_ACTIONS.length) % QUICK_ACTIONS.length);
    }
  };

  return (
    <div className="ui-overlay-enter fixed inset-0 z-50 flex items-start justify-center pt-28 bg-black/60">
      {/* Click outside to close */}
      <div className="absolute inset-0" onClick={onClose} />

      <div ref={dialogRef} role="dialog" aria-modal="true" aria-label="Spotlight command prompt" tabIndex={-1} className="ui-panel-enter relative w-full max-w-xl mx-4 overflow-hidden rounded-xl border border-[var(--stroke-nous)] bg-[var(--ui-bg-elevated)] shadow-lg">
        {/* Top Input Bar */}
        <div className="flex items-center gap-3 border-b border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-chrome)] px-4 py-3.5">
          <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-elevated)]">
            <AetherLogo size={16} />
          </div>
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="Ask AetherAgent anything or quick command... (Alt+Space)"
            className="flex-1 bg-transparent text-sm text-[var(--ui-text-primary)] placeholder:text-[var(--ui-text-quaternary)] outline-none"
          />
          {query ? (
            <button
              type="button"
              onClick={() => void handleSubmit()}
              className="flex items-center gap-1 rounded bg-[var(--ui-accent)] px-2.5 py-1 text-xs font-medium text-[var(--ui-accent-fg)] hover:opacity-90"
            >
              <span>Ask</span>
              <CornerDownLeft size={11} />
            </button>
          ) : (
            <span className="rounded bg-[var(--ui-control-fill)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--ui-text-tertiary)]">
              ESC to close
            </span>
          )}
        </div>

        {/* Quick Action List */}
        <div className="p-2 space-y-1">
          <div className="px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider text-[var(--ui-text-quaternary)]">
            Suggested Actions
          </div>
          {QUICK_ACTIONS.map((action, idx) => {
            const Icon = action.icon;
            const isSelected = selectedIndex === idx && !query;
            return (
              <button
                key={action.id}
                type="button"
                onClick={() => void handleSubmit(action.prompt)}
                onMouseEnter={() => setSelectedIndex(idx)}
                className={`flex w-full items-center justify-between rounded-xl px-3 py-2 text-left text-xs transition-colors duration-100 hover:transition-none ${
                  isSelected
                    ? 'bg-[var(--ui-row-active)] text-[var(--ui-accent)]'
                    : 'text-[var(--ui-text-secondary)] hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)]'
                }`}
              >
                <div className="flex items-center gap-2.5">
                  <Icon size={14} className={isSelected ? 'text-[var(--ui-accent)]' : 'text-[var(--ui-text-tertiary)]'} />
                  <span>{action.label}</span>
                </div>
                <div className="flex items-center gap-1 text-[10px] text-[var(--ui-text-quaternary)]">
                  <span>Run</span>
                  <ArrowRight size={10} />
                </div>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
};
