import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { Brain, X } from 'lucide-react';
import { useModalAccessibility } from '../hooks/useModalAccessibility';
import type { SelfImprovementEvent } from '../types/agent';

interface SelfImprovementBadgeProps {
  review: SelfImprovementEvent;
}

function ReflectionSkillInspector({ runId, sessionId, skillName, onClose }: {
  runId: string;
  sessionId: string;
  skillName: string;
  onClose: () => void;
}) {
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const closeRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useModalAccessibility<HTMLElement>(true, onClose, closeRef);

  useEffect(() => {
    let active = true;
    setContent(null);
    setError(null);
    const load = async () => {
      try {
        const skill = await invoke<string>('read_reflection_skill', { runId, sessionId, skillName });
        if (active) setContent(skill);
      } catch (reason) {
        if (active) setError(reason instanceof Error ? reason.message : String(reason));
      }
    };
    void load();
    return () => { active = false; };
  }, [runId, sessionId, skillName, attempt]);

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-3 sm:p-4" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <section ref={dialogRef} role="dialog" aria-modal="true" aria-label={`Self-improvement skill: ${skillName}`} tabIndex={-1} className="flex max-h-[85vh] w-full min-w-0 max-w-3xl flex-col overflow-hidden rounded-xl border border-[#30363d] bg-[#161b22] text-[#e6edf3] shadow-2xl">
        <header className="flex shrink-0 items-start justify-between gap-3 border-b border-[#30363d] px-4 py-3">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold [overflow-wrap:anywhere]">{skillName}</h2>
            <p className="mt-1 text-[11px] text-[#8b949e] [overflow-wrap:anywhere]">{`.aether/skills/${skillName}/SKILL.md`}</p>
          </div>
          <button ref={closeRef} type="button" onClick={onClose} aria-label="Close skill inspector" className="icon-button"><X size={16} aria-hidden="true" /></button>
        </header>
        <div className="min-h-0 min-w-0 flex-1 overflow-auto p-4">
          {error !== null ? (
            <div className="space-y-3">
              <p role="alert" className="text-xs text-[#ff7b72] [overflow-wrap:anywhere]">Could not read SKILL.md: {error}</p>
              <button type="button" className="toolbar-button" onClick={() => setAttempt((current) => current + 1)}>Retry</button>
            </div>
          ) : content === null ? (
            <p role="status" className="text-xs text-[#8b949e]">Loading SKILL.md…</p>
          ) : (
            <pre className="whitespace-pre-wrap font-mono text-xs leading-5 text-[#c9d1d9] [overflow-wrap:anywhere]"><code>{content}</code></pre>
          )}
        </div>
      </section>
    </div>,
    document.body,
  );
}

export function SelfImprovementBadge({ review }: SelfImprovementBadgeProps) {
  const [open, setOpen] = useState(false);
  const noAction = review.action === 'no_action';
  const skillName = review.skill_name;
  useEffect(() => {
    if (noAction || !skillName?.trim()) setOpen(false);
  }, [noAction, skillName]);
  if (!noAction && !skillName?.trim()) return null;
  const label = noAction
    ? 'Self-improvement review · No reusable lesson found'
    : `Self-improvement review · Skill '${skillName}' ${review.action === 'skill_updated' ? 'updated' : 'created'}`;
  const content = (
    <>
      <Brain size={13} aria-hidden="true" className="mt-0.5 shrink-0 text-[#c0aec2]" />
      <span className="min-w-0 bg-gradient-to-r from-[#d8b6a4] to-[#c4b8d9] bg-clip-text text-transparent [overflow-wrap:anywhere]">{label}</span>
    </>
  );
  const rowClass = 'inline-flex min-w-0 max-w-full items-start gap-1.5 text-left text-[11px] leading-5';

  return noAction ? (
    <span className={rowClass}>{content}</span>
  ) : (
    <>
      <button type="button" aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen(true)} className={`${rowClass} border-0 bg-transparent p-0 hover:opacity-90`}>
        {content}
      </button>
      {open && skillName && <ReflectionSkillInspector key={JSON.stringify([review.run_id, review.session_id, skillName])} runId={review.run_id} sessionId={review.session_id} skillName={skillName} onClose={() => setOpen(false)} />}
    </>
  );
}
