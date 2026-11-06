import React, { useEffect, useRef, useState } from 'react';
import { BookOpen, Check, FileEdit, Plus, Save, X } from 'lucide-react';
import { useModalAccessibility } from '../hooks/useModalAccessibility';

interface MemoryModalProps {
  open: boolean;
  onClose: () => void;
  workspaceCwd?: string;
  onReadMemory: () => Promise<string | null>;
  onSaveMemory: (content: string) => Promise<void>;
  onInitMemory: () => Promise<string>;
}

export const MemoryModal: React.FC<MemoryModalProps> = ({
  open,
  onClose,
  workspaceCwd,
  onReadMemory,
  onSaveMemory,
  onInitMemory,
}) => {
  const [content, setContent] = useState<string | null>(null);
  const [originalContent, setOriginalContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [savedSuccess, setSavedSuccess] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const savedTimerRef = useRef<number | null>(null);

  useEffect(() => {
    if (!open) return;
    let active = true;
    setLoading(true);
    setSavedSuccess(false);
    setError(null);
    onReadMemory()
      .then((data) => {
        if (!active) return;
        setContent(data);
        setOriginalContent(data);
      })
      .catch((reason) => {
        if (!active) return;
        setContent(null);
        setOriginalContent(null);
        setError(`Could not read AGENTS.md: ${String(reason)}`);
      })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [open, onReadMemory]);

  useEffect(() => () => {
    if (savedTimerRef.current !== null) window.clearTimeout(savedTimerRef.current);
  }, []);

  const dirty = content !== null && originalContent !== null && content !== originalContent;
  const requestClose = () => {
    if (dirty && !window.confirm('Discard unsaved AGENTS.md changes?')) return;
    onClose();
  };
  const dialogRef = useModalAccessibility<HTMLDivElement>(open, requestClose);

  if (!open) return null;

  const handleSave = async () => {
    if (content === null) return;
    setSaving(true);
    setError(null);
    try {
      await onSaveMemory(content);
      setOriginalContent(content);
      setSavedSuccess(true);
      if (savedTimerRef.current !== null) window.clearTimeout(savedTimerRef.current);
      savedTimerRef.current = window.setTimeout(() => setSavedSuccess(false), 2000);
    } catch (reason) {
      setError(`Could not save AGENTS.md: ${String(reason)}`);
    } finally {
      setSaving(false);
    }
  };

  const handleInit = async () => {
    setLoading(true);
    setError(null);
    try {
      await onInitMemory();
      const updated = await onReadMemory();
      setContent(updated);
      setOriginalContent(updated);
    } catch (reason) {
      setError(`Could not initialize AGENTS.md: ${String(reason)}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4" onMouseDown={(event) => { if (event.target === event.currentTarget) requestClose(); }}>
      <div ref={dialogRef} role="dialog" aria-modal="true" aria-label="Workspace memory and instructions" tabIndex={-1} className="flex h-[80vh] w-full max-w-3xl flex-col rounded-2xl border border-[#30363d] bg-[#161b22] text-[#e6edf3] shadow-2xl overflow-hidden">
        {/* Header */}
        <div className="flex h-12 shrink-0 items-center justify-between border-b border-[#30363d] px-4 bg-[#0d1117]">
          <div className="flex items-center gap-2">
            <BookOpen size={16} className="text-[#f0883e]" />
            <h2 className="text-xs font-semibold uppercase tracking-wider text-[#e6edf3]">
              Workspace Memory & Instructions (AGENTS.md)
            </h2>
          </div>
          <button
            type="button"
            onClick={requestClose}
            aria-label="Close workspace memory"
            className="flex h-7 w-7 items-center justify-center rounded-md border border-[#30363d] bg-[#161b22] text-[#8b949e] hover:text-white"
          >
            <X size={14} />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 flex flex-col min-h-0 p-4">
          <div className="mb-2 text-xs text-[#8b949e]">
            Persistent instructions in <code className="font-mono text-[#58a6ff]">AGENTS.md</code> are automatically injected into every run to guide AetherAgent's coding style, stack conventions, and project rules.
          </div>

          {loading ? (
            <div className="flex-1 flex items-center justify-center text-xs text-[#6e7681]">
              Loading workspace memory...
            </div>
          ) : content !== null ? (
            <div className="flex-1 flex flex-col min-h-0 rounded-xl border border-[#30363d] bg-[#0d1117] p-2 focus-within:border-[#58a6ff]">
              <textarea
                value={content}
                onChange={(e) => setContent(e.target.value)}
                placeholder="Enter workspace instructions, rules, or project context in Markdown..."
                aria-label="AGENTS.md workspace instructions"
                className="flex-1 w-full bg-transparent p-2 font-mono text-xs text-[#e6edf3] outline-none resize-none leading-relaxed"
              />
            </div>
          ) : (
            <div className="flex-1 flex flex-col items-center justify-center border border-dashed border-[#30363d] rounded-xl p-8 text-center bg-[#0d1117]/50 space-y-3">
              <FileEdit size={32} className="text-[#6e7681]" />
              <div className="text-sm font-medium text-white">No AGENTS.md instructions found</div>
              <p className="max-w-md text-xs text-[#8b949e]">
                {workspaceCwd
                  ? `Your workspace (${workspaceCwd}) does not have an AGENTS.md instruction file yet. Generate one tailored to your tech stack.`
                  : 'Please open a workspace folder first to initialize AGENTS.md project memory.'}
              </p>
              {workspaceCwd && (
                <button
                  type="button"
                  onClick={handleInit}
                  className="flex items-center gap-2 rounded-lg border border-[#58a6ff]/50 bg-[#1f6feb]/20 px-4 py-2 text-xs font-semibold text-[#58a6ff] hover:bg-[#1f6feb]/35 transition-colors"
                >
                  <Plus size={14} /> Initialize AGENTS.md for this Workspace
                </button>
              )}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between border-t border-[#30363d] bg-[#0d1117] px-4 py-3">
          <div className="min-w-0 flex-1 text-[11px] text-[#6e7681]">
            {error && <span role="alert" className="block break-words text-[#ff7b72]">{error}</span>}
            {savedSuccess && <span className="text-[#3fb950] font-medium flex items-center gap-1"><Check size={12} /> Saved AGENTS.md successfully</span>}
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={requestClose}
              className="rounded-md border border-[#30363d] bg-[#161b22] px-3 py-1.5 text-xs text-[#8b949e] hover:text-white"
            >
              Close
            </button>
            {content !== null && (
              <button
                type="button"
                onClick={handleSave}
                disabled={saving}
                className="flex items-center gap-1.5 rounded-md border border-[#3fb950]/50 bg-[#238636] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#2ea043] transition-colors"
              >
                <Save size={13} /> {saving ? 'Saving...' : 'Save Instructions'}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
