import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { BookOpen, X } from 'lucide-react';
import type { SkillInfo } from '../types/agent';

interface SelectedSkillChipProps {
  skill: SkillInfo | undefined;
  name: string;
  onRemove: () => void;
}

export function SelectedSkillChip({ skill, name, onRemove }: SelectedSkillChipProps) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 8, top: 8 });
  const root = useRef<HTMLSpanElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const preview = useRef<HTMLDivElement>(null);
  const id = useId();
  const description = skill?.description.trim() || 'No description';

  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      if (!trigger.current || !preview.current) return;
      const anchor = trigger.current.getBoundingClientRect();
      const panel = preview.current.getBoundingClientRect();
      const above = anchor.top - panel.height - 6;
      const top = above >= 8 ? above : anchor.bottom + 6;
      setPosition({
        left: Math.max(8, Math.min(anchor.left, window.innerWidth - panel.width - 8)),
        top: Math.max(8, Math.min(top, window.innerHeight - panel.height - 8)),
      });
    };
    place();
    preview.current?.focus({ preventScroll: true });
    window.addEventListener('resize', place);
    document.addEventListener('scroll', place, true);
    return () => {
      window.removeEventListener('resize', place);
      document.removeEventListener('scroll', place, true);
    };
  }, [open, name, skill]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: Event) => {
      const target = event.target as Node;
      if (!root.current?.contains(target) && !preview.current?.contains(target)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      setOpen(false);
      trigger.current?.focus();
    };
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('focusin', outside);
    document.addEventListener('keydown', escape, true);
    return () => {
      document.removeEventListener('pointerdown', outside, true);
      document.removeEventListener('focusin', outside);
      document.removeEventListener('keydown', escape, true);
    };
  }, [open]);

  return <span ref={root} className="inline-flex min-w-0 items-center gap-1.5 rounded-md border border-[#30363d] bg-[#0d1117] px-2 py-1 text-[11px] text-[#a5d6ff]">
    <button ref={trigger} type="button" aria-label={`Preview skill ${name}`} title={description}
      aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? id : undefined}
      onClick={() => setOpen((previous) => !previous)}
      className="inline-flex min-w-0 items-center gap-1.5 rounded hover:text-[#e6edf3] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#58a6ff]">
      <BookOpen size={12} className="shrink-0" aria-hidden="true" /><span className="max-w-48 truncate">{name}</span>
    </button>
    <button type="button" aria-label={`Remove skill ${name}`} onClick={() => { setOpen(false); onRemove(); }}
      className="rounded text-[#8b949e] hover:text-[#e6edf3] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#58a6ff]">
      <X size={12} aria-hidden="true" />
    </button>
    {open && createPortal(
      <div ref={preview} id={id} role="dialog" aria-label={`Skill preview: ${name}`} tabIndex={-1}
        style={position}
        className="ui-menu fixed z-[60] max-h-[min(16rem,calc(100vh-1rem))] w-[min(22rem,calc(100vw-1rem))] overflow-y-auto rounded-xl border border-[#30363d] bg-[#161b22] p-3 text-xs text-[#e6edf3] shadow-xl focus:outline-none">
        <div className="mb-2 flex items-start justify-between gap-2">
          <span className="min-w-0 break-all font-mono font-medium text-[#a5d6ff]">{name}</span>
          <button type="button" aria-label="Close skill preview" onClick={() => { setOpen(false); trigger.current?.focus(); }}
            className="shrink-0 rounded p-0.5 text-[#8b949e] hover:text-[#e6edf3] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#58a6ff]">
            <X size={14} aria-hidden="true" />
          </button>
        </div>
        <p className="whitespace-pre-wrap break-words leading-5">{description}</p>
        <dl className="mt-3 space-y-1 text-[11px]">
          <dt className="text-[#8b949e]">Source</dt>
          <dd className="break-all">{skill?.source.trim() || 'Source unavailable'}</dd>
          <dt className="pt-1 text-[#8b949e]">Path</dt>
          <dd className="break-all font-mono">{skill?.path.trim() || 'Path unavailable'}</dd>
        </dl>
      </div>, document.body)}
  </span>;
}
