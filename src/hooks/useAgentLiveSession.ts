import type { SessionData, SessionWindow, SkillInfo, SubagentProgress } from '../types/agent';

export type RunPhase = 'preparing_attachments' | 'waiting_provider' | 'receiving_answer' | 'running_tool' | 'awaiting_approval' | 'saving_results' | 'completed' | 'cancelled' | 'error';
export interface RunOutcome {
  answer: 'completed' | 'cancelled' | 'error';
  /** Undefined means a command or external tool may have changed files. */
  filesChanged?: boolean;
  verification: 'not_run' | 'passed' | 'failed' | 'unknown';
}
export interface RunStatus {
  runId: string | null;
  sessionId: string;
  origin: 'chat' | 'council';
  phase: RunPhase;
  startedAt: number;
  phaseSince: number;
  endedAt?: number;
  outcome?: RunOutcome;
  saveFailed?: boolean;
}

/** A live session outlives its projection in the currently viewed chat. */
export interface LiveSession {
  data: SessionData;
  /** UI runtime only; never serialized into session history. */
  runStatus: RunStatus | null;
  /** Last immutable snapshot acknowledged by native storage, never the dirty stream. */
  committedData?: SessionData;
  dirty: boolean;
  /** Invalidates queued snapshots after a native history replacement. */
  historyEpoch: number;
  persistenceBusy: number;
  nativeWindow?: SessionWindow;
  loadingOlder: boolean;
  historyError: string | null;
  runId: string | null;
  messageId: string | null;
  failed: boolean;
  stopRequested: boolean;
  /** Optimistic cancellation must not become a terminal journal lower bound. */
  cancellationPending: boolean;
  startedAt: number | null;
  text: { content: string; reasoning: string };
  council: { contentA: string; reasoningA: string; contentB: string; reasoningB: string };
  terminal: Record<string, string>;
  frame: number | null;
  checkpointTimer: ReturnType<typeof setTimeout> | null;
  checkpointBusy: boolean;
  /** Journal acknowledgements are not canonical commits. */
  deltaBusy: boolean;
  /** Run-owned changes only; lazy artifact reads never enter this set. */
  checkpointArtifactIds: Set<string>;
  checkpointSequence: number;
  nativeCheckpointRevision?: string;
  journalSnapshot?: { messages: SessionData['messages']; artifacts: SessionData['artifacts']; revision: string };
  checkpointGeneration: number;
  subagents: SubagentProgress[];
  skills: SkillInfo[];
  artifactId: string | null;
  authoritative: boolean;
  starting: boolean;
  cancelledBeforeStart: boolean;
}

export function liveSession(data: SessionData): LiveSession {
  return {
    data, runStatus: null, committedData: data.historyWindow ? data : undefined, dirty: false, historyEpoch: 0, persistenceBusy: 0, nativeWindow: data.historyWindow, loadingOlder: false, historyError: null, runId: null, messageId: null, failed: false, stopRequested: false, cancellationPending: false,
    startedAt: null, text: { content: '', reasoning: '' },
    council: { contentA: '', reasoningA: '', contentB: '', reasoningB: '' },
    terminal: {}, frame: null, checkpointTimer: null, checkpointBusy: false, deltaBusy: false, checkpointArtifactIds: new Set<string>(), checkpointSequence: 0, checkpointGeneration: 0,
    subagents: [], skills: [], artifactId: data.artifacts[0]?.id ?? null,
    authoritative: false, starting: false, cancelledBeforeStart: false,
  };
}
