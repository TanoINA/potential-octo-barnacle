import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { invoke } from '@tauri-apps/api/core';
import { Brain, X } from 'lucide-react';
import { useModalAccessibility } from '../hooks/useModalAccessibility';
import type { SelfImprovementChange, SelfImprovementEvent } from '../types/agent';

function changeLabel(change: SelfImprovementChange) {
  return change.action === 'skill_staged' ? 'staged · awaiting approval' : change.action === 'skill_updated' ? 'updated' : change.action === 'skill_created' ? 'created' : change.pending_id ? 'rejected' : 'no action';
}

interface SelfImprovementBadgeProps {
  review: SelfImprovementEvent;
}

interface ReviewProposal {
  id: string;
  skill_name: string;
  file_path: string;
  summary?: string;
}

interface ReviewProposalDiff extends ReviewProposal {
  before: string;
  after: string;
  diff: string;
}

function changeKey(change: SelfImprovementChange) {
  return JSON.stringify([change.pending_id, change.skill_name, change.file_path, change.skill_path]);
}

function ReflectionSkillInspector({ runId, sessionId, changes, pendingDecisions, onPendingChange, onResolve, onClose }: {
  runId: string;
  sessionId: string;
  changes: SelfImprovementChange[];
  pendingDecisions: Set<string>;
  onPendingChange: () => void;
  onResolve: (change: SelfImprovementChange, result: SelfImprovementChange | null) => void;
  onClose: () => void;
}) {
  const [selected, setSelected] = useState(0);
  const change = changes[selected] ?? changes[0];
  const skillName = change.skill_name;
  const filePath = change.file_path;
  const targetKey = changeKey(change);
  const staged = change.action === 'skill_staged';
  const rejected = change.action === 'no_action';
  const [content, setContent] = useState<string | null>(null);
  const [proposal, setProposal] = useState<ReviewProposalDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const mounted = useRef(false);
  const decisionKey = JSON.stringify([sessionId, proposal?.id]);
  const busy = pendingDecisions.has(decisionKey);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const generation = useRef(0);
  const closeRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useModalAccessibility<HTMLElement>(true, onClose, closeRef);

  useEffect(() => {
    const request = ++generation.current;
    setContent(null);
    setProposal(null);
    setError(null);
    const load = async () => {
      try {
        if (rejected) return;
        if (staged) {
          const proposals = await invoke<ReviewProposal[]>('list_review_proposals', { sessionId });
          if (generation.current !== request) return;
          const matches = proposals.filter((item) => item.skill_name === skillName && (!change.pending_id || item.id === change.pending_id));
          if (matches.length !== 1) throw new Error('Proposal is no longer available or is ambiguous.');
          const diff = await invoke<ReviewProposalDiff>('review_proposal_diff', { sessionId, id: matches[0].id });
          if (generation.current !== request) return;
          if (diff.id !== matches[0].id || diff.skill_name !== skillName) throw new Error('Proposal does not match the selected skill.');
          setProposal(diff);
          setContent(diff.diff);
        } else {
          const skill = await invoke<string>('read_reflection_skill', { runId, sessionId, skillName, ...(filePath ? { filePath } : {}) });
          if (generation.current === request) setContent(skill);
        }
      } catch (reason) {
        if (generation.current === request) setError(reason instanceof Error ? reason.message : String(reason));
      }
    };
    void load();
    return () => { ++generation.current; };
  }, [runId, sessionId, skillName, filePath, targetKey, change.action, change.pending_id, staged, rejected, attempt]);

  const resolveProposal = async (decision: 'approve' | 'reject') => {
    if (!staged || !proposal || pendingDecisions.has(decisionKey) || proposal.skill_name !== skillName || (change.pending_id && proposal.id !== change.pending_id)) return;
    pendingDecisions.add(decisionKey);
    onPendingChange();
    setError(null);
    const request = generation.current;
    try {
      const result = await invoke<SelfImprovementChange>(`${decision}_review_proposal`, { sessionId, id: proposal.id });
      if (generation.current !== request) return;
      onResolve(change, decision === 'approve' ? result : null);
    } catch (reason) {
      if (generation.current === request) setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      pendingDecisions.delete(decisionKey);
      onPendingChange();
      if (mounted.current && generation.current !== request) setAttempt((current) => current + 1);
    }
  };

  return createPortal(
    <div className="ui-overlay-enter fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-3 sm:p-4" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <section ref={dialogRef} role="dialog" aria-modal="true" aria-label={`Self-improvement skill: ${skillName}`} tabIndex={-1} className="ui-panel-enter flex max-h-[85vh] w-full min-w-0 max-w-3xl flex-col overflow-hidden rounded-xl border border-[#30363d] bg-[#161b22] text-[#e6edf3] shadow-lg">
        <header className="flex shrink-0 items-start justify-between gap-3 border-b border-[#30363d] px-4 py-3">
          <div className="min-w-0">
            <h2 className="text-sm font-semibold [overflow-wrap:anywhere]">{skillName}</h2>
            <p className="mt-1 text-[11px] text-[#8b949e] [overflow-wrap:anywhere]">{proposal?.file_path ?? change.file_path ?? change.skill_path ?? `.aether/skills/${skillName}/SKILL.md`}</p>
            {staged && <p className="mt-1 text-xs text-[#8b949e]">Staged proposal · awaiting approval</p>}
          </div>
          <button ref={closeRef} type="button" onClick={onClose} aria-label="Close skill inspector" className="icon-button"><X size={16} aria-hidden="true" /></button>
        </header>
        {changes.length > 1 && <nav aria-label="Review changes" className="flex shrink-0 flex-wrap gap-2 border-b border-[#30363d] p-3">
          {changes.map((item, index) => <button key={`${item.skill_name}-${index}`} type="button" aria-pressed={selected === index} className="toolbar-button" onClick={() => setSelected(index)}>{item.skill_name} · {changeLabel(item)}</button>)}
        </nav>}
        <div className="min-h-0 min-w-0 flex-1 overflow-auto p-4">
          {change.summary && <p className="mb-3 text-xs text-[#8b949e]">{change.summary}</p>}
          {error !== null ? (
            <div className="space-y-3">
              <p role="alert" className="text-xs text-[#ff7b72] [overflow-wrap:anywhere]">{staged ? 'Could not review proposal' : 'Could not read SKILL.md'}: {error}</p>
              <button type="button" className="toolbar-button" disabled={busy} onClick={() => setAttempt((current) => current + 1)}>Retry</button>
            </div>
          ) : rejected ? (
            <p role="status" className="text-xs text-[#8b949e]">Proposal rejected. No skill changes were applied.</p>
          ) : content === null ? (
            <p role="status" className="text-xs text-[#8b949e]">{staged ? 'Loading proposal diff…' : 'Loading SKILL.md…'}</p>
          ) : (
            <pre aria-label={staged ? 'Proposal diff' : 'SKILL.md'} className="whitespace-pre-wrap font-mono text-xs leading-5 text-[#c9d1d9] [overflow-wrap:anywhere]"><code>{content}</code></pre>
          )}
        </div>
        {staged && proposal && <footer className="flex shrink-0 gap-2 border-t border-[#30363d] p-3">
          <button type="button" className="toolbar-button" disabled={busy} onClick={() => { void resolveProposal('approve'); }}>Approve proposal</button>
          <button type="button" className="toolbar-button" disabled={busy} onClick={() => { void resolveProposal('reject'); }}>Reject proposal</button>
          {busy && <span role="status" className="text-xs text-[#8b949e]">Saving decision…</span>}
        </footer>}
      </section>
    </div>,
    document.body,
  );
}

export function SelfImprovementBadge({ review }: SelfImprovementBadgeProps) {
  const [open, setOpen] = useState(false);
  const pendingDecisions = useRef(new Set<string>());
  const [, setDecisionVersion] = useState(0);
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const scope = JSON.stringify([review.run_id, review.session_id, review.action, review.skill_name, review.changes]);
  const [resolved, setResolved] = useState<{ scope: string; changes: Record<string, SelfImprovementChange> }>({ scope, changes: {} });
  const noAction = review.action === 'no_action';
  const originalChanges: SelfImprovementChange[] = (review.changes?.length ? review.changes : review.skill_name ? [{ action: review.action, skill_name: review.skill_name }] : []).filter((change) => change.skill_name.trim());
  const changes = originalChanges.map((change) => (resolved.scope === scope ? resolved.changes[changeKey(change)] : undefined) ?? change);
  const skillName = changes[0]?.skill_name;
  useEffect(() => {
    if (noAction || !skillName) setOpen(false);
  }, [noAction, skillName]);
  if (!noAction && !skillName) return null;
  const stagedCount = changes.filter((change) => change.action === 'skill_staged').length;
  const label = noAction
    ? 'Self-improvement review · No reusable lesson found'
    : review.changes?.length
      ? `Self-improvement review · ${changes.length} ${changes.length === 1 ? 'change' : 'changes'}${stagedCount ? ` · ${stagedCount} staged · awaiting approval` : ''}${review.summary ? ` · ${review.summary}` : ''}`
      : `Self-improvement review · Skill '${skillName}' ${changeLabel(changes[0])}`;
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
      {open && skillName && <ReflectionSkillInspector key={scope} runId={review.run_id} sessionId={review.session_id} changes={changes} pendingDecisions={pendingDecisions.current} onPendingChange={() => {
        if (mounted.current) setDecisionVersion((current) => current + 1);
      }} onResolve={(change, result) => {
        const resolvedChange: SelfImprovementChange = result
          ? { ...change, ...result, pending_id: undefined }
          : { ...change, action: 'no_action', pending_id: change.pending_id ?? change.skill_name };
        setResolved((current) => ({ scope, changes: { ...(current.scope === scope ? current.changes : {}), [changeKey(change)]: resolvedChange } }));
      }} onClose={() => setOpen(false)} />}
    </>
  );
}
