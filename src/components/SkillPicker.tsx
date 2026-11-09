import { useMemo, useRef, useState } from 'react';
import { BookOpen, Check, Search, X } from 'lucide-react';
import type { SkillInfo } from '../types/agent';

interface SkillPickerProps {
  skills: SkillInfo[];
  selected: string[];
  loading?: boolean;
  error?: string | null;
  onToggle: (name: string) => void;
  onClose: () => void;
}

export function SkillPicker({ skills, selected, loading, error, onToggle, onClose }: SkillPickerProps) {
  const [query, setQuery] = useState('');
  const root = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const matches = useMemo(() => {
    const term = query.trim().toLowerCase();
    return skills.filter((skill) => `${skill.name} ${skill.description}`.toLowerCase().includes(term));
  }, [skills, query]);

  return <div ref={root} role="dialog" aria-label="Choose skills" onKeyDown={(event) => {
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); onClose(); }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    const items = Array.from(root.current?.querySelectorAll<HTMLButtonElement>('[data-skill-option]:not(:disabled)') ?? []);
    if (!items.length) return;
    event.preventDefault();
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    if (index === 0 && event.key === 'ArrowUp') search.current?.focus();
    else items[(index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length]?.focus();
  }} className="ui-menu absolute bottom-9 left-0 z-30 flex w-[min(24rem,calc(100vw-3rem))] flex-col overflow-hidden rounded-xl border border-[#30363d] bg-[#161b22] shadow-xl">
    <div className="flex items-center justify-between px-3 pt-3 text-xs text-[#e6edf3]">
      <span className="font-medium">Skills for next message</span>
      <button type="button" onClick={onClose} aria-label="Close skills" className="rounded p-1 text-[#8b949e] hover:text-[#e6edf3]"><X size={14} /></button>
    </div>
    <p className="px-3 pb-2 text-[11px] text-[#8b949e]">Loads instructions directly. Your message stays unchanged.</p>
    <div className="mx-3 mb-2 flex items-center gap-2 rounded-lg border border-[#30363d] bg-[#0d1117] px-2">
      <Search size={13} className="shrink-0 text-[#8b949e]" />
      <input ref={search} autoFocus type="search" aria-label="Search skills" placeholder="Search skills..." value={query} onChange={(event) => setQuery(event.target.value)} className="min-w-0 flex-1 bg-transparent py-2 text-xs text-[#e6edf3] outline-none" />
    </div>
    <div className="max-h-[min(18rem,35vh)] overflow-y-auto p-1.5">
      {loading ? <p role="status" className="px-2 py-3 text-xs text-[#8b949e]">Loading skills...</p>
        : error ? <p role="alert" className="px-2 py-3 text-xs text-[#f85149]">{error}</p>
        : matches.length === 0 ? <p className="px-2 py-3 text-xs text-[#8b949e]">{skills.length ? 'No matching skills' : 'No skills detected'}</p>
        : matches.map((skill) => {
          const checked = selected.includes(skill.name);
          return <button data-skill-option key={skill.name} type="button" aria-pressed={checked} disabled={!checked && selected.length >= 5} onClick={() => onToggle(skill.name)} title={`${skill.description}\n${skill.path}`} className="flex w-full items-start gap-2 rounded-lg px-2 py-2 text-left hover:bg-[#21262d] focus-visible:bg-[#21262d] focus-visible:outline-none disabled:opacity-40">
            <BookOpen size={14} className="mt-0.5 shrink-0 text-[#8b949e]" />
            <span className="min-w-0 flex-1"><span className="block truncate font-mono text-xs text-[#a5d6ff]">{skill.name}</span><span className="mt-0.5 line-clamp-2 block text-[11px] leading-4 text-[#8b949e]">{skill.description || 'No description'}</span></span>
            {checked && <Check size={14} className="mt-0.5 shrink-0 text-[#58a6ff]" />}
          </button>;
        })}
    </div>
    <div className="flex justify-between border-t border-[#30363d] px-3 py-2 text-[10px] text-[#8b949e]"><span>{selected.length}/5 selected</span><span>Escape to close</span></div>
  </div>;
}
