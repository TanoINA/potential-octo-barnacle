import type { SessionData, SkillInfo, SubagentProgress } from '../types/agent';

/** A live session outlives its projection in the currently viewed chat. */
export interface LiveSession {
  data: SessionData;
  runId: string | null;
  messageId: string | null;
  failed: boolean;
  stopRequested: boolean;
  startedAt: number | null;
  text: { content: string; reasoning: string };
  council: { contentA: string; reasoningA: string; contentB: string; reasoningB: string };
  terminal: Record<string, string>;
  frame: number | null;
  checkpointTimer: ReturnType<typeof setTimeout> | null;
  checkpointBusy: boolean;
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
    data, runId: null, messageId: null, failed: false, stopRequested: false,
    startedAt: null, text: { content: '', reasoning: '' },
    council: { contentA: '', reasoningA: '', contentB: '', reasoningB: '' },
    terminal: {}, frame: null, checkpointTimer: null, checkpointBusy: false, checkpointGeneration: 0,
    subagents: [], skills: [], artifactId: data.artifacts[0]?.id ?? null,
    authoritative: false, starting: false, cancelledBeforeStart: false,
  };
}
