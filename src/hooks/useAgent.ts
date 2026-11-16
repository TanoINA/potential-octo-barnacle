import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import { useHistoryPreview } from './useHistoryPreview';
import { pruneSessionCache, recordSessionLoad, scheduleSessionIndexWarmup } from '../utils/sessionCacheBudget';
import { liveSession, type LiveSession, type RunPhase, type RunOutcome, type RunStatus } from './useAgentLiveSession';
import { playTaskCompleteChime, showDesktopNotification } from '../utils/audioChime';
import type {
  AgentEvent,
  BackgroundTask,
  MessageSegment,
  ToolCallItem,
  AgentRunRequest,
  AgentSettings,
  ChatMessage,
  EffectiveRunConfig,
  GitRepoStatus,
  ProviderConfig,
  ReasoningEffort,
  Session,
  SessionActivity,
  SessionData,
  SessionPage,
  SessionWindow,
  SelfImprovementEvent,
  SessionModelOverride,
  ExecutionMode,
} from '../types/agent';

function makeId(prefix: string): string {
  const value = typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  return `${prefix}_${value}`;
}

function createSession(workspaceCwd?: string): SessionData {
  const now = Date.now();
  return {
    session: {
      id: makeId('sess'),
      title: 'New Chat',
      createdAt: now,
      updatedAt: now,
      messageCount: 0,
      workspaceCwd,
    },
    messages: [],
    artifacts: [],
    modelOverride: null,
  };
}

function cancelledMessage(message: ChatMessage): ChatMessage {
  const cancelTool = (tool: ToolCallItem): ToolCallItem =>
    tool.status === 'running' || tool.status === 'pending'
      ? { ...tool, status: 'cancelled' as const }
      : tool;

  return {
    ...message,
    isStreaming: false,
    notice: null,
    runStatus: 'cancelled',
    toolCalls: message.toolCalls?.map(cancelTool),
    segments: message.segments?.map((seg) => ({
      ...seg,
      toolCalls: seg.toolCalls?.map(cancelTool),
    })),
    councilTurn: message.councilTurn ? {
      ...message.councilTurn,
      candidateA: { ...message.councilTurn.candidateA, isStreaming: false },
      candidateB: { ...message.councilTurn.candidateB, isStreaming: false },
    } : undefined,
  };
}

function splitLegacyPreamble(content: string): { preamble: string; content: string } | null {
  if (!content || typeof content !== 'string') return null;
  const transitionRegex = /(Dua subagent selesai|Subagent selesai|Hasil tes|Semua pengujian selesai|Deliverables|All tasks completed|Investigation complete|Test results)/i;
  const match = content.match(transitionRegex);
  if (match && match.index && match.index > 30) {
    let splitPos = match.index;
    const before = content.slice(0, splitPos);
    const dotIdx = before.lastIndexOf('.');
    if (dotIdx > 30 && splitPos - dotIdx <= 2) {
      splitPos = dotIdx + 1;
    }
    const preamble = content.slice(0, splitPos).trim();
    const rest = content.slice(splitPos).trim();
    if (preamble.length > 20 && rest.length > 20) {
      return { preamble, content: rest };
    }
  }
  return null;
}

function extractPreview(messages: ChatMessage[]): string | undefined {
  if (!messages || !Array.isArray(messages)) return undefined;
  const target = [...messages].reverse().find((m) => (m.content || m.preamble || '').trim().length > 0);
  if (!target) return undefined;
  return (target.content || target.preamble || '').slice(0, 240);
}

// Cache normally contains the latest review, but IPC may miss a native approval.
// A resolved pending ID must never be replaced by its stale staged counterpart.
function mergeReviewStatus(native: SelfImprovementEvent | undefined, cached: SelfImprovementEvent | undefined): SelfImprovementEvent | undefined {
  if (!cached) return native;
  if (!native || native.run_id !== cached.run_id || native.session_id !== cached.session_id) return cached;
  let changed = false;
  const changes = cached.changes?.map((change) => {
    if (!change.pending_id || change.action !== 'skill_staged') return change;
    const resolved = native.changes?.find((candidate) => candidate.pending_id === change.pending_id && candidate.action !== 'skill_staged');
    if (!resolved) return change;
    changed = true;
    return resolved;
  });
  if (!changed) return cached;
  const active = changes?.find((change) => change.action !== 'no_action');
  return { ...cached, changes, action: active?.action ?? 'no_action', skill_name: active?.skill_name };
}

function restoreSession(loaded: SessionData, reviews: Map<string, SelfImprovementEvent>): SessionData {
  return {
    ...loaded,
    messages: loaded.messages.map((message) => {
      const restored = message.role === 'assistant' && (message.isStreaming || (message.runId !== undefined && !message.runStatus))
        ? cancelledMessage(message) : { ...message, isStreaming: false };
      const review = mergeReviewStatus(message.selfImprovement, reviews.get(JSON.stringify([loaded.session.id, message.runId ?? message.id])));
      const safeContent = restored.content || '';
      let withPreamble = { ...restored, content: safeContent };
      if (!withPreamble.preamble && withPreamble.toolCalls && withPreamble.toolCalls.length > 0 && withPreamble.content) {
        const split = splitLegacyPreamble(withPreamble.content);
        if (split) {
          withPreamble = { ...withPreamble, preamble: split.preamble, content: split.content };
        }
      }
      if (!withPreamble.segments && withPreamble.role === 'assistant') {
        if (withPreamble.preamble && withPreamble.toolCalls && withPreamble.toolCalls.length > 0) {
          withPreamble.segments = [
            { id: 'seg_0', text: withPreamble.preamble, toolCalls: withPreamble.toolCalls },
            { id: 'seg_1', text: withPreamble.content, toolCalls: [] },
          ];
        } else if (withPreamble.toolCalls && withPreamble.toolCalls.length > 0) {
          withPreamble.segments = [
            { id: 'seg_0', text: '', toolCalls: withPreamble.toolCalls },
            { id: 'seg_1', text: withPreamble.content, toolCalls: [] },
          ];
        } else if (withPreamble.content) {
          withPreamble.segments = [
            { id: 'seg_0', text: withPreamble.content, toolCalls: [] },
          ];
        }
      }
      return review ? { ...withPreamble, selfImprovement: review } : withPreamble;
    }),
  };
}

function resolveEffective(settings: AgentSettings | null, override?: SessionModelOverride | null): EffectiveRunConfig | null {
  if (!settings) return null;
  const providerId = override?.providerId || settings.activeProviderId;
  const base = settings.providers.find((provider) => provider.id === providerId)
    || settings.providers.find((provider) => provider.id === settings.activeProviderId)
    || settings.providers[0];
  if (!base) return null;
  return {
    provider: { ...base, model: override?.model || base.model },
    thinking: override?.thinking ?? settings.thinking,
    reasoningEffort: override?.reasoningEffort ?? settings.reasoningEffort,
    usesSettingsDefault: !override || Object.values(override).every((value) => value === undefined),
  };
}

function normalizeSelectedSkills(value: unknown): string[] | undefined {
  if (value === undefined) return undefined;
  if (!Array.isArray(value)) throw new Error('Selected skills must be a list of skill names.');
  if (value.length > 5) throw new Error('Select at most five skills for the next turn.');
  const names = new Set<string>();
  for (const entry of value) {
    if (typeof entry !== 'string') throw new Error('Selected skill names must be strings.');
    const trimmed = entry.trim();
    // Match reflection::validate_name, including ASCII-only case normalization.
    if (trimmed.length > 64 || !/^[A-Za-z0-9]+(?:-[A-Za-z0-9]+)*$/.test(trimmed)) {
      throw new Error('Selected skill names must be ASCII slugs of at most 64 characters.');
    }
    const name = trimmed.toLowerCase();
    if (/^(con|prn|aux|nul|com[1-9]|lpt[1-9])$/.test(name)) throw new Error('Selected skill names cannot be Windows device names.');
    names.add(name);
  }
  return [...names];
}

function hasExactKeys(value: unknown, keys: string[]): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).length === keys.length && keys.every((key) => Object.prototype.hasOwnProperty.call(value, key));
}

async function recoverCommittedSave(error: unknown, data: SessionData): Promise<SessionPage> {
  const message = error instanceof Error ? error.message : String(error);
  const prefix = '[canonical-committed]';
  if (!message.startsWith(prefix)) throw error;
  let receipt;
  try {
    receipt = JSON.parse(message.slice(prefix.length));
  } catch {
    throw error;
  }
  if (!hasExactKeys(receipt, ['proof', 'cause']) || typeof receipt.cause !== 'string'
    || !hasExactKeys(receipt.proof, ['version', 'sessionId', 'canonicalSha256', 'offset', 'total', 'firstMessageId'])) throw error;
  const proof = receipt.proof;
  const offset = data.historyWindow?.offset ?? 0;
  // Match the original submitted suffix, not the possibly advanced live view.
  // Native alone verifies the hash against canonical bytes; no full load here.
  if (proof.version !== 1 || proof.sessionId !== data.session.id
    || typeof proof.canonicalSha256 !== 'string' || !/^[0-9a-f]{64}$/.test(proof.canonicalSha256)
    || !Number.isSafeInteger(proof.offset) || proof.offset !== offset || offset < 0
    || !Number.isSafeInteger(proof.total) || proof.total !== offset + data.messages.length
    || proof.firstMessageId !== (data.messages[0]?.id ?? null)) throw error;
  return invoke<SessionPage>('recover_save_ack', { sessionId: data.session.id, proof });
}

export function useAgent() {
  const preview = useHistoryPreview();
  const [projection, setProjection] = useState<{ data: SessionData; runStatus: RunStatus | null }>(() => ({ data: createSession(), runStatus: null }));
  const { data, runStatus } = projection;
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionActivities, setSessionActivities] = useState<Record<string, SessionActivity>>({});
  const sessionActivitiesRef = useRef(sessionActivities);
  const [settings, setSettings] = useState<AgentSettings | null>(null);
  const [workspaceCwd, setWorkspaceCwd] = useState('');
  const [isStreaming, setIsStreaming] = useState(false);
  const [isReviewing, setIsReviewing] = useState(false);
  const runFailedRef = useRef(false);
  const [stopRequested, setStopRequested] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
  const [isSessionLoading, setIsSessionLoading] = useState(false);
  const [loadingOlderMessages, setLoadingOlderMessages] = useState(false);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const publishedHistoryRef = useRef({ loadingOlder: false, error: null as string | null });
  const [activeArtifactId, setActiveArtifactId] = useState<string | null>(null);
  const [lastError, setLastError] = useState<string | null>(null);
  const [gitStatus, setGitStatus] = useState<GitRepoStatus | null>(null);
  const [subagents, setSubagents] = useState<import('../types/agent').SubagentProgress[]>([]);
  const [activeSkills, setActiveSkills] = useState<import('../types/agent').SkillInfo[]>([]);
  const [backgroundTasks, setBackgroundTasks] = useState<BackgroundTask[]>([]);

  const dataRef = useRef(data);
  const settingsRef = useRef(settings);
  const confirmedSettingsRef = useRef(settings);
  const settingsRevisionRef = useRef(0);
  const settingsQueueRef = useRef<Promise<void>>(Promise.resolve());
  const workspaceRef = useRef(workspaceCwd);
  const sessionIdRef = useRef(data.session.id);
  const currentRunRef = useRef<string | null>(null);
  const streamingMessageIdRef = useRef<string | null>(null);
  const runStartTimeRef = useRef<number | null>(null);
  const persistenceQueueRef = useRef<Promise<void>>(Promise.resolve());
  const staleSaveErrorsRef = useRef(new Map<string, string>());
  const navigationRef = useRef(0);
  const sessionOperationRef = useRef<boolean | symbol>(false);
  const initializingRef = useRef(true);
  const navigationPendingRef = useRef(false);
  const reviewResultsRef = useRef(new Map<string, SelfImprovementEvent>());
  const manualRenamesRef = useRef(new Set<string>());
  const liveSessionsRef = useRef(new Map<string, LiveSession>());
  const viewedLive = useCallback(() => {
    let live = liveSessionsRef.current.get(sessionIdRef.current);
    if (!live) {
      live = liveSession(dataRef.current);
      liveSessionsRef.current.set(sessionIdRef.current, live);
    }
    return live;
  }, []);

  // Map order tracks visits. Dirty, queued and running sessions are pinned.
  const pruneCache = useCallback(() => {
    pruneSessionCache(liveSessionsRef.current, sessionIdRef.current);
  }, []);

  const publishLive = useCallback((live: LiveSession) => {
    const id = live.data.session.id;
    const phase = live.runStatus?.phase;
    const activity: SessionActivity | undefined = !phase ? undefined : phase === 'error' ? 'failed'
      : phase === 'saving_results' ? 'saving' : phase === 'completed' || phase === 'cancelled' ? phase : 'running';
    if (sessionActivitiesRef.current[id] !== activity) {
      const next = { ...sessionActivitiesRef.current };
      if (activity) next[id] = activity;
      else delete next[id];
      sessionActivitiesRef.current = next;
      setSessionActivities(next);
    }
    if (sessionIdRef.current !== id) return;
    dataRef.current = live.data;
    currentRunRef.current = live.runId;
    streamingMessageIdRef.current = live.messageId;
    runStartTimeRef.current = live.startedAt;
    runFailedRef.current = live.failed;
    setProjection({ data: live.data, runStatus: live.runStatus });
    // History status changes independently of streamed content. Do not add
    // setters to every frame when these values have not changed.
    const history = publishedHistoryRef.current;
    if (history.loadingOlder !== live.loadingOlder) {
      history.loadingOlder = live.loadingOlder;
      setLoadingOlderMessages(live.loadingOlder);
    }
    if (history.error !== live.historyError) {
      history.error = live.historyError;
      setHistoryError(live.historyError);
    }
    setIsStreaming(live.runId !== null);
    setStopRequested(live.stopRequested);
    setSubagents(live.subagents);
    setActiveSkills(live.skills);
    setActiveArtifactId(live.artifactId);
  }, []);

  const transitionRun = useCallback((live: LiveSession, phase: RunPhase) => {
    if (!live.runStatus || live.runStatus.phase === phase) return;
    live.runStatus = { ...live.runStatus, phase, phaseSince: Date.now() };
  }, []);

  const mutateData = useCallback((mutator: (current: SessionData) => SessionData, target?: LiveSession) => {
    const live = target ?? viewedLive();
    const next = mutator(live.data);
    // Mutator memakai data imutabel; identitas sama tidak perlu dipublikasikan.
    if (next === live.data) return;
    live.data = next;
    live.authoritative = true;
    live.dirty = true;
    publishLive(live);
  }, [publishLive, viewedLive]);

  const selectArtifact = useCallback((value: string | null | ((current: string | null) => string | null)) => {
    const live = viewedLive();
    live.artifactId = typeof value === 'function' ? value(live.artifactId) : value;
    publishLive(live);
    const artifactId = live.artifactId;
    const navigation = navigationRef.current;
    const baseline = live.data;
    if (artifactId && !live.data.artifacts.some((artifact) => artifact.id === artifactId)) {
      void invoke<import('../types/agent').Artifact>('read_session_artifact', { sessionId: live.data.session.id, artifactId }).then((artifact) => {
        if (navigationRef.current !== navigation || sessionIdRef.current !== live.data.session.id || live.artifactId !== artifactId) return;
        if (live.data.artifacts.some((item) => item.id === artifactId)) return;
        // Cache hydration is not a user edit: never add checkpointArtifactIds
        // or replace streamed artifacts.
        if (!live.data.historyWindow && live.data !== baseline) return;
        live.data = { ...live.data, artifacts: [...live.data.artifacts, artifact] };
        publishLive(live);
      }).catch((error) => {
        if (navigationRef.current === navigation && sessionIdRef.current === live.data.session.id && live.artifactId === artifactId) setLastError('Could not load artifact: ' + String(error));
      });
    }
  }, [publishLive, viewedLive]);

  const refreshSessions = useCallback(async () => {
    try {
      setSessions(await invoke<Session[]>('list_sessions'));
    } catch (error) {
      setLastError(`Could not list sessions: ${String(error)}`);
    }
  }, []);

  const refreshBackgroundTasks = useCallback(async () => {
    try {
      const sessionId = sessionIdRef.current;
      const navigation = navigationRef.current;
      const list = await invoke<BackgroundTask[]>('list_background_tasks', { sessionId });
      if (sessionIdRef.current === sessionId && navigationRef.current === navigation) {
        setBackgroundTasks((list || []).filter((task) => task.sessionId === sessionId));
      }
    } catch {
      // background tasks not available or desktop mock
    }
  }, []);

  const killBackgroundTask = useCallback(async (taskId: string) => {
    const sessionId = sessionIdRef.current;
    const navigation = navigationRef.current;
    const stillViewed = () => sessionIdRef.current === sessionId && navigationRef.current === navigation;
    try {
      await invoke('kill_background_task', { sessionId, taskId });
      if (stillViewed()) await refreshBackgroundTasks();
    } catch (err) {
      if (stillViewed()) setLastError(`Failed to kill background task: ${String(err)}`);
    }
  }, [refreshBackgroundTasks]);

  const persistData = useCallback((value: SessionData, checkpoint = false, reportError?: () => boolean): Promise<void> => {
    const navigation = navigationRef.current;
    const shouldReportError = reportError ?? (() => navigationRef.current === navigation && sessionIdRef.current === value.session.id);
    const live = liveSessionsRef.current.get(value.session.id);
    const historyEpoch = live?.historyEpoch;
    if (live) live.persistenceBusy += 1;
    const queued = persistenceQueueRef.current.then(async () => {
      try {
        if (live && live.historyEpoch !== historyEpoch) {
          throw new Error('History was compacted; stale queued snapshot was not saved.');
        }
        const { historyWindow, ...snapshot } = value;
        // Reviews and checkpoints may have been queued before metadata changed.
        // Preserve current metadata, but keep message/artifact contents and cursor
        // from this snapshot (never turn a partial snapshot into a full write).
        const nativeData = live ? {
          ...snapshot,
          modelOverride: live.data.modelOverride,
          session: {
            ...snapshot.session,
            title: live.data.session.title,
            workspaceCwd: live.data.session.workspaceCwd,
            pinned: live.data.session.pinned,
            pinnedAt: live.data.session.pinnedAt,
          },
        } : snapshot;
        if (historyWindow) {
          // Refresh only the revision: offset and boundary belong to this immutable snapshot.
          const window = { ...historyWindow, revision: live?.nativeWindow?.revision ?? historyWindow.revision };
          let saved: SessionWindow;
          let committed = nativeData;
          try {
            saved = await invoke<SessionWindow>(checkpoint ? 'checkpoint_session_page' : 'save_session_page', { data: nativeData, window });
          } catch (error) {
            const page = await recoverCommittedSave(error, value);
            saved = page.window;
            committed = page.data;
          }
          if (live) {
            live.nativeWindow = saved;
            live.nativeCheckpointRevision = saved.revision;
            live.journalSnapshot = undefined;
            // The write can finish after streaming has advanced. Acknowledge
            // exactly what native received, independently of the current UI.
            live.committedData = { ...committed, historyWindow: saved };
          }
          if (live?.data === value) {
            live.data = { ...value, historyWindow: saved };
            live.dirty = false;
            // A revision acknowledgement is metadata, not a content update.
            // Keep the next operation's cursor current without republishing
            // the snapshot or adding work to the checkpoint hot path.
            if (sessionIdRef.current === value.session.id && dataRef.current === value) {
              dataRef.current = live.data;
            }
          }
        } else {
          let recovered: SessionPage | undefined;
          try {
            await invoke<void>(checkpoint ? 'checkpoint_session' : 'save_session', { data: nativeData });
          } catch (error) {
            recovered = await recoverCommittedSave(error, value);
          }
          if (live) {
            live.nativeWindow = undefined;
            live.committedData = recovered?.data ?? nativeData;
            live.journalSnapshot = undefined;
            live.nativeCheckpointRevision = recovered?.window.revision;
            // Full saves return void. Read only the revision, never adopt this
            // one-row page as the UI's loaded history window.
            if (live.runId && !recovered) {
              const page = await invoke<SessionPage>('load_session_page', { sessionId: value.session.id, limit: 1 });
              live.nativeCheckpointRevision = page.window.revision;
            }
          }
          if (live?.data === value) live.dirty = false;
        }
        const staleError = staleSaveErrorsRef.current.get(value.session.id);
        if (staleError && shouldReportError()) setLastError((current) => current === staleError ? null : current);
        staleSaveErrorsRef.current.delete(value.session.id);
        if (!checkpoint) await refreshSessions();
      } catch (error) {
        if (shouldReportError()) {
          const message = 'Could not save session: ' + String(error);
          if (/stale.*(cursor|revision|window)|(cursor|revision|window).*stale/i.test(String(error))) staleSaveErrorsRef.current.set(value.session.id, message);
          setLastError(message);
        }
        throw error;
      } finally {
        if (live) live.persistenceBusy -= 1;
        pruneCache();
      }
    });
    persistenceQueueRef.current = queued.catch(() => undefined);
    return queued;
  }, [refreshSessions, pruneCache]);

  const finishRun = useCallback((live: LiveSession, outcome: RunOutcome) => {
    if (!live.runStatus) return;
    live.runStatus = { ...live.runStatus, phase: 'saving_results', phaseSince: Date.now(), outcome };
    const saving = live.runStatus;
    publishLive(live);
    void persistFinal();
    async function persistFinal() {
      try {
        await persistData(live.data);
        if (live.runStatus !== saving) return;
        live.runStatus = { ...saving, phase: outcome.answer, phaseSince: Date.now(), endedAt: Date.now() };
      } catch {
        if (live.runStatus !== saving) return;
        live.runStatus = { ...saving, phase: 'error', phaseSince: Date.now(), endedAt: Date.now(), saveFailed: true };
      }
      publishLive(live);
    }
  }, [persistData, publishLive]);

  // Only known native mutations may reconcile a stale revision. Native must
  // validate the whole supplied tail/artifacts, not merely the first message ID.
  // Never use this as a generic save-conflict retry.
  const refreshNativeWindow = useCallback(async (live: LiveSession, baseline: SessionData, allowMetadata: boolean, reviewRunId?: string) => {
    const { historyWindow, ...snapshot } = baseline;
    if (!historyWindow) {
      // New sessions intentionally remain nonpaged. A known metadata mutation
      // still invalidates their independent checkpoint baseline revision.
      if (live.runId) {
        const page = await invoke<SessionPage>('load_session_page', { sessionId: baseline.session.id, limit: 1 });
        live.nativeCheckpointRevision = page.window.revision;
      }
      live.committedData = { ...baseline, session: { ...baseline.session,
        title: live.data.session.title, pinned: live.data.session.pinned, pinnedAt: live.data.session.pinnedAt,
      } };
      return;
    }
    const window = { ...historyWindow, revision: live.nativeWindow?.revision ?? historyWindow.revision };
    const refreshed = await invoke<SessionWindow>('refresh_session_window', {
      sessionId: baseline.session.id, data: snapshot, window, allowMetadata, reviewRunId,
    });
    // Missing IPC responses never authorize an upgrade. Acknowledgements update
    // internal metadata only, without republishing messages or cursor state.
    if (refreshed && live.data.historyWindow) {
      live.nativeWindow = { ...live.data.historyWindow, revision: refreshed.revision };
      live.nativeCheckpointRevision = refreshed.revision;
      live.committedData = {
        ...baseline,
        ...(allowMetadata ? { session: {
          ...baseline.session,
          title: live.data.session.title,
          pinned: live.data.session.pinned,
          pinnedAt: live.data.session.pinnedAt,
        } } : {}),
        historyWindow: { ...historyWindow, revision: refreshed.revision },
      };
    }
  }, []);

  const flushPending = useCallback((target?: LiveSession) => {
    const live = target ?? viewedLive();
    if (live.frame !== null) {
      cancelAnimationFrame(live.frame);
      live.frame = null;
    }
    const text = live.text;
    const terminal = live.terminal;
    const council = live.council;
    if (
      !text.content &&
      !text.reasoning &&
      !council.contentA &&
      !council.reasoningA &&
      !council.contentB &&
      !council.reasoningB &&
      Object.keys(terminal).length === 0
    ) {
      return;
    }
    live.text = { content: '', reasoning: '' };
    live.terminal = {};
    live.council = { contentA: '', reasoningA: '', contentB: '', reasoningB: '' };
    const mutate = (fn: (current: SessionData) => SessionData) => mutateData(fn, live);
    mutate((current) => {
      let messages = current.messages;
      const messageId = live.messageId;
      if (messageId && (text.content || text.reasoning)) {
        messages = messages.map((message) => {
          if (message.id !== messageId) return message;
          const segments: MessageSegment[] = message.segments ? [...message.segments] : [{ id: 'seg_0', text: '', toolCalls: [] }];
          if (text.content) {
            const lastIdx = segments.length - 1;
            const last = segments[lastIdx];
            if (last && last.toolCalls && last.toolCalls.length > 0) {
              segments.push({
                id: `seg_${segments.length}`,
                text: text.content,
                toolCalls: [],
              });
            } else if (last) {
              segments[lastIdx] = {
                ...last,
                text: (last.text || '') + text.content,
              };
            }
          }
          return {
            ...message,
            content: (message.content || '') + text.content,
            reasoning: (message.reasoning || '') + text.reasoning,
            segments,
          };
        });
      }
      if (messageId && (council.contentA || council.reasoningA || council.contentB || council.reasoningB)) {
        messages = messages.map((message) => {
          if (message.id !== messageId || !message.councilTurn) return message;
          return {
            ...message,
            councilTurn: {
              ...message.councilTurn,
              candidateA: {
                ...message.councilTurn.candidateA,
                content: message.councilTurn.candidateA.content + council.contentA,
                reasoning: (message.councilTurn.candidateA.reasoning || '') + council.reasoningA,
              },
              candidateB: {
                ...message.councilTurn.candidateB,
                content: message.councilTurn.candidateB.content + council.contentB,
                reasoning: (message.councilTurn.candidateB.reasoning || '') + council.reasoningB,
              },
            },
          };
        });
      }
      let artifacts = current.artifacts;
      const terminalEntries = Object.entries(terminal);
      if (terminalEntries.length > 0) {
        artifacts = current.artifacts.map((artifact) => {
          const delta = terminal[artifact.id];
          return delta ? { ...artifact, content: artifact.content + delta } : artifact;
        });
      }
      return { ...current, messages, artifacts };
    });
  }, [mutateData, viewedLive]);

  const loadOlderMessages = useCallback(async (): Promise<void> => {
    const live = viewedLive();
    const cursor = live.data.historyWindow;
    if (!cursor || cursor.offset <= 0 || live.loadingOlder || navigationPendingRef.current || sessionOperationRef.current) return;
    const sessionId = live.data.session.id;
    const navigation = navigationRef.current;
    const firstId = live.data.messages[0]?.id;
    const isOrigin = () => sessionIdRef.current === sessionId && navigationRef.current === navigation
      && live.data.historyWindow?.offset === cursor.offset && live.data.messages[0]?.id === firstId;
    const started = performance.now();
    const retry = live.historyError !== null;
    live.loadingOlder = true;
    live.historyError = null;
    publishLive(live);
    try {
      // A page read folds the native journal. Commit the visible suffix first
      // so the read cannot silently invalidate an active checkpoint revision.
      if (live.runId && (live.journalSnapshot || live.deltaBusy)) {
        flushPending(live);
        await persistData(live.data);
        if (!isOrigin()) return;
      }
      const revision = live.nativeWindow?.revision ?? cursor.revision;
      if (retry) {
        // A matching boundary ID does not prove the loaded tail is unchanged.
        // Acquire a new revision only with a complete canonical replacement of
        // the loaded suffix; never authorize a write of stale local contents.
        const baseline = live.data;
        const nativeWindow = live.nativeWindow;
        const conflict = 'History changed. Finish or stop the current work and reload the session before saving.';
        const assertClean = () => {
          if (live.dirty || live.runId || live.persistenceBusy || live.checkpointBusy
            || live.data !== baseline || live.nativeWindow !== nativeWindow
            || sessionOperationRef.current || navigationPendingRef.current) throw new Error(conflict);
        };
        assertClean();
        const latest = await invoke<SessionPage>('load_session_page', { sessionId, limit: 50 });
        if (!isOrigin()) return;
        assertClean();
        const freshRevision = latest.window.revision;
        const total = latest.window.total;
        // Retain the prior ordinal boundary and at least the loaded message
        // count when history shrinks, then include one newly requested page.
        // Stable message IDs within this suffix remain available as anchors.
        const target = Math.max(0, Math.min(cursor.offset, total - baseline.messages.length) - 50);
        const pages = [latest];
        let offset = latest.window.offset;
        if (offset < 0 || offset + latest.data.messages.length !== total) {
          throw new Error('History changed while reloading the latest page.');
        }
        while (offset > target) {
          const limit = Math.min(50, offset - target);
          const page = await invoke<SessionPage>('load_session_page', { sessionId, before: offset, limit, revision: freshRevision });
          if (!isOrigin()) return;
          assertClean();
          if (page.window.revision !== freshRevision || page.window.total !== total
            || page.window.offset !== offset - limit || page.data.messages.length !== limit) {
            throw new Error('History changed while reloading older messages.');
          }
          pages.push(page);
          offset = page.window.offset;
        }
        const messages = pages.reverse().flatMap((page) => page.data.messages).slice(target - offset);
        const artifacts = new Map(pages.flatMap((page) => page.data.artifacts).map((artifact) => [artifact.id, artifact]));
        const window: SessionWindow = { offset: target, total, revision: freshRevision, firstMessageId: messages[0]?.id ?? null };
        const canonicalData: SessionData = { ...latest.data, messages, artifacts: [...artifacts.values()], historyWindow: window };
        const restored = restoreSession(canonicalData, reviewResultsRef.current);
        // Commit contents and revision together, after every bounded read and
        // origin/clean check succeeds. Failure leaves both old values intact.
        // UI restoration may overlay cached reviews that native has not stored.
        live.committedData = canonicalData;
        live.data = restored;
        live.nativeWindow = window;
        return;
      }
      const page = await invoke<SessionPage>('load_session_page', { sessionId, before: cursor.offset, limit: 50, revision });
      if (!isOrigin()) return;
      const restored = restoreSession(page.data, reviewResultsRef.current);
      const current = live.data;
      const seen = new Set(current.messages.map((message) => message.id));
      const older = restored.messages.filter((message) => {
        if (seen.has(message.id)) return false;
        seen.add(message.id);
        return true;
      });
      const messages = [...older, ...current.messages];
      const artifacts = new Map(restored.artifacts.map((artifact) => [artifact.id, artifact]));
      for (const artifact of current.artifacts) artifacts.set(artifact.id, artifact);
      const window: SessionWindow = {
        ...page.window,
        total: page.window.offset + messages.length,
        revision: live.nativeWindow?.revision !== revision ? live.nativeWindow?.revision ?? page.window.revision : page.window.revision,
        firstMessageId: messages[0]?.id ?? null,
      };
      live.nativeWindow = window;
      // Loading is not an edit; retain dirty status and never replace the tail.
      live.data = { ...current, messages, artifacts: [...artifacts.values()], historyWindow: window };
    } catch (error) {
      if (isOrigin()) live.historyError = 'Could not load older messages: ' + String(error) + ' Retry loading older messages.';
    } finally {
      if (sessionIdRef.current === sessionId && navigationRef.current === navigation && !live.historyError) recordSessionLoad({ kind: 'older', durationMs: performance.now() - started, loaded: live.data.messages.length, total: live.data.historyWindow?.total ?? live.data.messages.length, at: Date.now() });
      live.loadingOlder = false;
      publishLive(live);
      pruneCache();
    }
  }, [viewedLive, publishLive, pruneCache, flushPending, persistData]);

  // One bounded journal request per live session, independent of RAF. Only
  // active assistant messages are serialized; user images never enter this IPC.
  const scheduleCheckpoint = useCallback((live: LiveSession) => {
    const generation = live.checkpointGeneration;
    const runId = live.runId;
    const sameArtifacts = (a: SessionData['artifacts'][number], b: SessionData['artifacts'][number]) =>
      a.id === b.id && a.content === b.content && a.title === b.title && a.type === b.type
      && a.language === b.language && a.filePath === b.filePath && a.timestamp === b.timestamp
      && (a.streamRevision ?? 0) === (b.streamRevision ?? 0);
    const arm = () => {
      if (live.cancellationPending || live.checkpointGeneration !== generation || !runId || live.runId !== runId || live.checkpointTimer !== null) return;
      live.checkpointTimer = setTimeout(() => {
        live.checkpointTimer = null;
        if (live.cancellationPending || live.checkpointGeneration !== generation || live.runId !== runId) return;
        if (live.deltaBusy || live.starting) { arm(); return; }
        flushPending(live);
        const snapshot = live.data;
        const messages = snapshot.messages.filter((message) => message.role === 'assistant' && message.runId === runId);
        if (!messages.length) return;
        live.deltaBusy = live.checkpointBusy = true;
        let retry = false;
        const queued = persistenceQueueRef.current.then(async () => {
          if (live.cancellationPending || live.checkpointGeneration !== generation || live.runId !== runId) return;
          const revision = live.nativeCheckpointRevision ?? live.nativeWindow?.revision;
          if (!revision) throw new Error('Missing checkpoint baseline revision');
          const previous = live.journalSnapshot?.revision === revision ? live.journalSnapshot : undefined;
          const baseline = previous ?? live.committedData;
          const oldArtifacts = new Map(baseline?.artifacts.map((artifact) => [artifact.id, artifact]));
          const artifacts = snapshot.artifacts.filter((artifact) => {
            if (!live.checkpointArtifactIds.has(artifact.id)) return false;
            const old = oldArtifacts.get(artifact.id);
            return !old || !sameArtifacts(artifact, old);
          });
          const oldMessages = previous?.messages ?? baseline?.messages.filter((message) => message.role === 'assistant' && message.runId === runId);
          if (!artifacts.length && JSON.stringify(messages) === JSON.stringify(oldMessages)) return;
          const ack = await invoke<{ revision: string; sequence: number }>('checkpoint_session_delta', {
            sessionId: snapshot.session.id,
            request: { runId, baseRevision: revision, sequence: ++live.checkpointSequence, messages, artifacts },
          });
          if (live.checkpointGeneration !== generation || live.runId !== runId) return;
          live.nativeCheckpointRevision = ack.revision;
          if (live.nativeWindow) live.nativeWindow = { ...live.nativeWindow, revision: ack.revision };
          // Keep a journal-only baseline. Never clear dirty or acknowledge the
          // full session: canonical storage still contains the initial text.
          // Keep run-owned IDs: a newer artifact object may already be live.
          // The immutable snapshot baseline skips acknowledged versions only.
          live.journalSnapshot = { messages, artifacts: snapshot.artifacts, revision: ack.revision };
        });
        persistenceQueueRef.current = queued.catch(() => undefined);
        void queued.catch((error) => {
          retry = true;
          if (live.checkpointGeneration === generation && live.runId === runId && sessionIdRef.current === snapshot.session.id) {
            setLastError('Could not checkpoint session: ' + String(error));
          }
        }).finally(() => {
          live.deltaBusy = live.checkpointBusy = false;
          if (retry || live.data !== snapshot || live.text.content || live.text.reasoning
            || live.council.contentA || live.council.reasoningA || live.council.contentB || live.council.reasoningB
            || Object.keys(live.terminal).length) arm();
        });
      }, 2000);
    };
    arm();
  }, [flushPending]);

  const scheduleFlush = useCallback((live: LiveSession) => {
    if (live.frame === null) live.frame = requestAnimationFrame(() => flushPending(live));
  }, [flushPending]);

  useEffect(() => {
    let active = true;
    const navigationAtStart = navigationRef.current;
    setIsLoading(true);
    setIsSessionLoading(true);
    void Promise.allSettled([
      invoke<AgentSettings>('get_settings'),
      invoke<Session[]>('list_sessions'),
    ]).then(async ([settingsResult, sessionsResult]) => {
      if (!active) return;
      if (settingsResult.status === 'fulfilled') {
        settingsRef.current = settingsResult.value;
        confirmedSettingsRef.current = settingsResult.value;
        setSettings(settingsResult.value);
      } else {
        setLastError(`Backend settings unavailable: ${String(settingsResult.reason)}`);
      }
      if (sessionsResult.status === 'fulfilled') {
        const sessionList = sessionsResult.value;
        setSessions(sessionList);
        // If sessions exist, restore the most recent session & its workspace
        if (sessionList.length > 0) {
          const latest = sessionList[0];
          const started = performance.now();
          await invoke<SessionPage>('load_session_page', { sessionId: latest.id, limit: 50 })
            .then(async (page) => {
              if (!active || navigationRef.current !== navigationAtStart) return;
              const normalized = restoreSession({ ...page.data, historyWindow: page.window }, reviewResultsRef.current);
              const live = liveSession(normalized);
              liveSessionsRef.current.set(normalized.session.id, live);
              sessionIdRef.current = normalized.session.id;
              publishLive(live);
              recordSessionLoad({ kind: 'cold', durationMs: performance.now() - started, loaded: normalized.messages.length, total: page.window.total, at: Date.now() });
              const sessCwd = normalized.session.workspaceCwd || '';
              setWorkspaceCwd(sessCwd);
              workspaceRef.current = sessCwd;
              await invoke('set_workspace_cwd', { newCwd: sessCwd });
            })
            .catch((error) => { if (active && navigationRef.current === navigationAtStart) setLastError(`Could not restore session: ${String(error)}`); });
        } else {
          // Fresh start without sessions: no workspace by default
          setWorkspaceCwd('');
          workspaceRef.current = '';
          invoke('set_workspace_cwd', { newCwd: '' }).catch(() => {});
        }
      }
      if (active) {
        initializingRef.current = false;
        setIsLoading(false);
        if (navigationRef.current === navigationAtStart) setIsSessionLoading(false);
      }
    });
    return () => { active = false; };
  }, [mutateData, publishLive]);

  useEffect(() => {
    if (isLoading || isSessionLoading || isStreaming || isReviewing) return;
    return scheduleSessionIndexWarmup(
      () => !initializingRef.current && !navigationPendingRef.current && !sessionOperationRef.current
        && [...liveSessionsRef.current.values()].every((live) => !live.runId && !live.starting && !live.persistenceBusy && !live.checkpointBusy && !live.deltaBusy && !live.loadingOlder),
      () => invoke<{ remaining: number; errors: number }>('warm_session_index', { limit: 3, budgetMs: 75 }),
    );
  }, [isLoading, isSessionLoading, isStreaming, isReviewing]);

  useEffect(() => {
    const unlistenPromise = listen<AgentEvent>('agent-event', ({ payload }) => {
      const scope = payload.data;
      const live = liveSessionsRef.current.get(scope.sessionId);
      if (!live || scope.runId !== live.runId) return;
      const mutate = (fn: (current: SessionData) => SessionData) => mutateData(fn, live);
      const updateSubagents = (fn: (prev: import('../types/agent').SubagentProgress[]) => import('../types/agent').SubagentProgress[]) => { live.subagents = fn(live.subagents); };
      const updateSkills = (fn: import('../types/agent').SkillInfo[] | ((prev: import('../types/agent').SkillInfo[]) => import('../types/agent').SkillInfo[])) => { live.skills = typeof fn === 'function' ? fn(live.skills) : fn; };
      switch (payload.type) {
        case 'provider_state':
          mutate((current) => ({
            ...current,
            messages: current.messages.map((message) => message.id === live.messageId
              ? { ...message, providerMessages: payload.data.providerMessages }
              : message),
          }));
          break;
        case 'notice': {
          transitionRun(live, 'waiting_provider');
          const noticeData = payload.data;
          if (noticeData.resetPartial) {
            // Retry abandons reasoning only; flush buffered narrative unchanged.
            live.text.reasoning = '';
            flushPending(live);
          }
          mutate((current) => ({
            ...current,
            messages: current.messages.map((message) => {
              if (message.id !== live.messageId) return message;
              return {
                ...message,
                checkpointEpoch: noticeData.resetPartial ? (message.checkpointEpoch ?? 0) + 1 : message.checkpointEpoch,
                reasoning: noticeData.resetPartial ? '' : message.reasoning,
                notice: noticeData.message ? {
                  message: noticeData.message,
                  attempt: noticeData.attempt,
                  maxAttempts: noticeData.maxAttempts,
                } : null,
              };
            }),
          }));
          break;
        }
        case 'chunk':
          transitionRun(live, 'receiving_answer');
          live.text.content += payload.data.text;
          scheduleFlush(live);
          if (live.data.messages.find((m) => m.id === live.messageId)?.notice) {
            mutate((current) => ({
              ...current,
              messages: current.messages.map((m) => m.id === live.messageId ? { ...m, notice: null } : m),
            }));
          }
          // Buffer menunggu frame; notice tetap hilang segera, checkpoint tetap aktif.
          scheduleCheckpoint(live);
          return;
        case 'reasoning':
          transitionRun(live, 'receiving_answer');
          live.text.reasoning += payload.data.text;
          scheduleFlush(live);
          if (live.data.messages.find((m) => m.id === live.messageId)?.notice) {
            mutate((current) => ({
              ...current,
              messages: current.messages.map((m) => m.id === live.messageId ? { ...m, notice: null } : m),
            }));
          }
          // Buffer menunggu frame; notice tetap hilang segera, checkpoint tetap aktif.
          scheduleCheckpoint(live);
          return;
        case 'tool_update':
          flushPending(live);
          mutate((current) => ({
            ...current,
            messages: current.messages.map((message) => {
              if (message.id !== live.messageId) return message;
              let preamble = message.preamble;
              let content = message.content;
              // If this message doesn't have a preamble yet, any text streamed before tools is the preamble
              if (preamble === undefined && (content || '').trim().length > 0) {
                preamble = content;
                content = '';
              }
              const incomingTool = payload.data.tool;
              const tools = [...(message.toolCalls || [])];
              const index = tools.findIndex((tool) => tool.id === incomingTool.id);
              const existingStartedAt = index >= 0 ? tools[index].startedAt : undefined;
              const normalizedTool = {
                ...incomingTool,
                startedAt: incomingTool.startedAt || existingStartedAt || Date.now(),
              };
              if (index >= 0) tools[index] = normalizedTool;
              else tools.push(normalizedTool);

              const segments: MessageSegment[] = message.segments ? [...message.segments] : [{ id: 'seg_0', text: '', toolCalls: [] }];
              const lastIdx = Math.max(0, segments.length - 1);
              const lastSeg = segments[lastIdx] || { id: `seg_${lastIdx}`, text: '', toolCalls: [] };
              const segTools = [...(lastSeg.toolCalls || [])];
              const segIdx = segTools.findIndex((tool) => tool.id === incomingTool.id);
              if (segIdx >= 0) segTools[segIdx] = normalizedTool;
              else segTools.push(normalizedTool);
              segments[lastIdx] = { ...lastSeg, toolCalls: segTools };

              return { ...message, preamble, content, toolCalls: tools, segments };
            }),
          }));
          const tools = live.data.messages.find(m => m.id === live.messageId)?.toolCalls ?? [];
          transitionRun(live, tools.some(tool => tool.status === 'running') ? 'running_tool' : payload.data.tool.status === 'pending' ? 'receiving_answer' : 'waiting_provider');
          if (payload.data.tool.name === 'run_command' || payload.data.tool.name === 'manage_task') {
            void refreshBackgroundTasks();
          }
          break;
        case 'artifact': {
          const artifact = payload.data.artifact;
          live.checkpointArtifactIds.add(artifact.id);
          if (artifact.type === 'terminal' && artifact.content.length > 0) {
            delete live.terminal[artifact.id];
          }
          mutate((current) => {
            const artifacts = [...current.artifacts];
            const index = artifacts.findIndex((item) => item.id === artifact.id);
            const existing = index >= 0 ? artifacts[index] : undefined;
            // Native journals freeze identity metadata; event revisions replace content only.
            const next = { ...(existing ?? artifact), content: artifact.content, streamRevision: (existing?.streamRevision ?? 0) + 1 };
            if (index >= 0) artifacts[index] = next;
            else artifacts.unshift(next);
            return { ...current, artifacts };
          });
          live.artifactId = artifact.id;
          break;
        }
        case 'terminal_output':
          if (payload.data.delta) live.checkpointArtifactIds.add(payload.data.artifactId);
          live.terminal[payload.data.artifactId] = (live.terminal[payload.data.artifactId] || '') + payload.data.delta;
          scheduleFlush(live);
          scheduleCheckpoint(live);
          return;
        case 'subagent_update': {
          const sub = payload.data.subagent;
          updateSubagents((prev) => {
            const index = prev.findIndex((item) => item.id === sub.id);
            if (index >= 0) {
              const next = [...prev];
              next[index] = sub;
              return next;
            }
            return [...prev, sub];
          });
          break;
        }
        case 'skill_activated': {
          const skill = payload.data.skill;
          updateSkills((prev) => {
            if (prev.some((item) => item.name.toLowerCase() === skill.name.toLowerCase())) return prev;
            return [...prev, skill];
          });
          break;
        }
        case 'council_chunk': {
          transitionRun(live, 'receiving_answer');
          const { candidate, kind, text: chunkText } = payload.data;
          if (candidate === 'A') {
            if (kind === 'reasoning') live.council.reasoningA += chunkText;
            else live.council.contentA += chunkText;
          } else {
            if (kind === 'reasoning') live.council.reasoningB += chunkText;
            else live.council.contentB += chunkText;
          }
          scheduleFlush(live);
          scheduleCheckpoint(live);
          return;
        }
        case 'council_done': {
          const { candidate, durationMs } = payload.data;
          mutate((current) => ({
            ...current,
            messages: current.messages.map((m) => {
              if (m.id !== live.messageId || !m.councilTurn) return m;
              if (candidate === 'A') {
                return {
                  ...m,
                  councilTurn: {
                    ...m.councilTurn,
                    candidateA: {
                      ...m.councilTurn.candidateA,
                      isStreaming: false,
                      durationMs,
                    },
                  },
                };
              } else {
                return {
                  ...m,
                  councilTurn: {
                    ...m.councilTurn,
                    candidateB: {
                      ...m.councilTurn.candidateB,
                      isStreaming: false,
                      durationMs,
                    },
                  },
                };
              }
            }),
          }));
          break;
        }
        case 'error':
          live.failed = true;
          transitionRun(live, 'error');
          if (sessionIdRef.current === scope.sessionId) setLastError(payload.data.message);
          mutate((current) => ({
            ...current,
            messages: current.messages.map((message) => {
              if (message.id !== live.messageId) return message;
              return { ...message, notice: null };
            }),
          }));
          break;
        case 'done': {
          live.checkpointGeneration += 1;
          if (live.checkpointTimer !== null) { clearTimeout(live.checkpointTimer); live.checkpointTimer = null; }
          flushPending(live);
          const elapsedMs = live.startedAt ? Date.now() - live.startedAt : 0;
          live.startedAt = null;
          const currentSettings = settingsRef.current;
          if (elapsedMs > 2500 && !payload.data.cancelled && payload.data.successful !== false && !live.failed) {
            if (currentSettings?.notificationsEnabled !== false && typeof document !== 'undefined' && document.hidden) {
              // Preview the assistant reply only — never echo the user's own prompt.
              const assistantMsg = [...live.data.messages].reverse().find((m) => m.role === 'assistant' && (m.content || m.preamble || '').trim());
              const previewText = (assistantMsg?.content || assistantMsg?.preamble || '').slice(0, 100);
              void showDesktopNotification('AetherAgent: Task Complete', previewText || 'Agent has completed execution.');
            }
            if (currentSettings?.soundChimeEnabled === true) {
              void playTaskCompleteChime();
            }
          }
          if (payload.data.cancelled) {
            updateSubagents((prev) =>
              prev.map((s) =>
                s.status === 'running'
                  ? { ...s, status: 'cancelled', completedAt: s.completedAt || Date.now() }
                  : s
              )
            );
          }
          mutate((current) => ({
            ...current,
            session: {
              ...current.session,
              updatedAt: Date.now(),
              messageCount: (current.historyWindow?.offset ?? 0) + current.messages.length,
              preview: extractPreview(current.messages),
            },
            messages: current.messages.map((message) => {
              if (message.id !== live.messageId) return message;
              if (payload.data.cancelled) {
                return cancelledMessage(message);
              }
              return {
                ...message,
                isStreaming: false,
                notice: null,
                runStatus: payload.data.successful === false || live.failed ? 'failed' : 'completed',
              };
            }),
          }));
          updateSkills([]);
          void refreshBackgroundTasks();

          const tools = live.data.messages.find(m => m.id === live.messageId)?.toolCalls ?? [];
          const applied = tools.some(tool => tool.status === 'done' && (tool.filesChanged?.length ?? 0) > 0);
          const untracked = tools.some(tool => !['read_file', 'list_directory', 'search_files', 'search_web', 'fetch_web_page'].includes(tool.name));
          const outcome: RunOutcome = {
            answer: payload.data.cancelled ? 'cancelled' : payload.data.successful === false || live.failed ? 'error' : 'completed',
            filesChanged: applied ? true : untracked ? undefined : false,
            verification: untracked ? 'unknown' : 'not_run',
          };
          // File diffs establish changes, never verification.
          if (applied && tools.every(tool => ['write_file', 'edit_file', 'read_file', 'list_directory', 'search_files'].includes(tool.name))) outcome.verification = 'not_run';
          live.cancellationPending = false;
          live.stopRequested = false;
          live.starting = false;
          live.runId = null;
          live.messageId = null;
          finishRun(live, outcome);
          void refreshGitStatus();

          // Auto-generate topic title after first completed turn (only if title has not been manually renamed)
          const userMsgs = live.data.messages.filter((m) => m.role === 'user');
          const sid = live.data.session.id;
          if (
            !manualRenamesRef.current.has(sid) &&
            userMsgs.length === 1 &&
            !payload.data.cancelled &&
            payload.data.successful !== false &&
            !live.failed
          ) {
            const firstUser = userMsgs[0];
            const promptText = (firstUser.content || '').trim();
            const firstAssistant = live.data.messages.find(
              (m) => m.role === 'assistant' && (m.content || m.preamble || '').trim()
            );
            const replySnippet = (firstAssistant?.content || firstAssistant?.preamble || '').trim().slice(0, 300);
            const currentProvider = resolveEffective(settingsRef.current, live.data.modelOverride)?.provider;

            if (promptText && currentProvider) {
              void invoke<string>('generate_session_title', {
                sessionId: sid,
                provider: currentProvider,
                prompt: promptText,
                responseSnippet: replySnippet || undefined,
              }).then((newTitle) => {
                if (newTitle && typeof newTitle === 'string' && newTitle.trim()) {
                  const cleanTitle = newTitle.trim();
                  mutate((curr) => {
                    if (curr.session.id === sid && !manualRenamesRef.current.has(sid)) {
                      return {
                        ...curr,
                        session: { ...curr.session, title: cleanTitle },
                      };
                    }
                    return curr;
                  });
                  void refreshSessions();
                }
              }).catch(() => {
                // Non-blocking fallback: keeps original prompt snippet
              });
            }
          }
          break;
        }
      }
      if (live.runId) scheduleCheckpoint(live);
      publishLive(live);
    });
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
      for (const live of liveSessionsRef.current.values()) {
        live.checkpointGeneration += 1;
        if (live.frame !== null) { cancelAnimationFrame(live.frame); live.frame = null; }
        if (live.checkpointTimer !== null) { clearTimeout(live.checkpointTimer); live.checkpointTimer = null; }
      }
    };
  }, [flushPending, mutateData, persistData, scheduleFlush, scheduleCheckpoint, publishLive, transitionRun, finishRun]);

  useEffect(() => {
    const unlisten = listen<SelfImprovementEvent>('agent:self-improvement', ({ payload }) => {
      const key = JSON.stringify([payload.session_id, payload.run_id]);
      const live = liveSessionsRef.current.get(payload.session_id);
      const existing = live?.data.messages.find((message) => message.role === 'assistant' && (message.runId === payload.run_id || message.id === payload.run_id));
      // Late staged IPC must not undo pending resolutions in either cache or live state.
      const merged = mergeReviewStatus(existing?.selfImprovement, mergeReviewStatus(reviewResultsRef.current.get(key), payload)) ?? payload;
      reviewResultsRef.current.delete(key);
      reviewResultsRef.current.set(key, merged);
      if (reviewResultsRef.current.size > 256) {
        const oldest = reviewResultsRef.current.keys().next().value;
        if (oldest !== undefined) reviewResultsRef.current.delete(oldest);
      }
      if (!live) return;
      if (existing) mutateData((current) => ({
        ...current,
        messages: current.messages.map((message) => message.role === 'assistant' && (message.runId === payload.run_id || message.id === payload.run_id)
          ? { ...message, selfImprovement: merged }
          : message),
      }), live);
      // Events can precede native's best-effort write or follow persistence.
      // Reserve reconciliation synchronously, then retain the save/merge for both.
      const reconciled = persistenceQueueRef.current.then(async () => {
        // Pending writes may acknowledge a different snapshot before this runs.
        const baseline = live.committedData;
        if (baseline) await refreshNativeWindow(live, baseline, false, payload.run_id);
      });
      persistenceQueueRef.current = reconciled.catch(() => undefined);
      if (existing) void persistData(live.data).catch(() => undefined);
    });
    const unlistenErrors = listen<{ run_id: string; session_id: string; message: string }>('agent:self-improvement-error', ({ payload }) => {
      if (payload.session_id === sessionIdRef.current) setLastError(`Self-improvement review failed: ${payload.message}`);
    });
    return () => {
      void unlisten.then((dispose) => dispose());
      void unlistenErrors.then((dispose) => dispose());
    };
  }, [mutateData, persistData, refreshNativeWindow]);

  const reviewSession = useCallback(async (): Promise<boolean> => {
    const effective = resolveEffective(settingsRef.current, dataRef.current.modelOverride);
    if (!effective || currentRunRef.current || sessionOperationRef.current) {
      setLastError(!effective ? 'Configure a provider before reviewing.' : 'Wait for the current run or session operation to finish.');
      return false;
    }
    const latest = dataRef.current.messages[dataRef.current.messages.length - 1];
    if (!latest || latest.role !== 'assistant' || latest.isStreaming
      || (latest.runId !== undefined && latest.runStatus !== 'completed')
      || (latest.runStatus !== undefined && latest.runStatus !== 'completed')
      || latest.toolCalls?.some((tool) => tool.status === 'running' || tool.status === 'pending' || tool.status === 'cancelled')) {
      setLastError('Complete a conversational turn before reviewing.');
      return false;
    }
    const sessionId = dataRef.current.session.id;
    sessionOperationRef.current = true;
    setIsReviewing(true);
    setLastError(null);
    try {
      await persistData(dataRef.current);
      await invoke('review_session', { sessionId, provider: effective.provider });
      return true;
    } catch (error) {
      if (sessionIdRef.current === sessionId) setLastError(`Self-improvement review failed: ${String(error)}`);
      return false;
    } finally {
      sessionOperationRef.current = false;
      setIsReviewing(false);
    }
  }, [persistData]);

  const effectiveConfig = useMemo(
    () => resolveEffective(settings, data.modelOverride),
    [settings, data.modelOverride],
  );

  const sendMessage = useCallback(async (
    rawPrompt: string,
    attachments?: import('../types/agent').AttachedContext[],
    options?: {
      council?: boolean;
      candidateA?: { provider: ProviderConfig; thinking?: boolean; reasoningEffort?: ReasoningEffort };
      candidateB?: { provider: ProviderConfig; thinking?: boolean; reasoningEffort?: ReasoningEffort };
      executionMode?: ExecutionMode;
      imageInputs?: import('../types/agent').ImageInput[];
      selectedSkills?: string[];
    },
  ) => {
    const prompt = (rawPrompt || '').trim();
    if (initializingRef.current || navigationPendingRef.current) {
      setLastError('Wait for local state to finish loading.');
      return false;
    }
    let selectedSkills: string[] | undefined;
    try {
      selectedSkills = normalizeSelectedSkills(options?.selectedSkills);
    } catch (error) {
      setLastError(String(error));
      return false;
    }
    if (!selectedSkills?.length && /^\/(review|reflect|refine)$/i.test(prompt)) return reviewSession();
    const currentSettings = settingsRef.current;
    const effective = resolveEffective(currentSettings, dataRef.current.modelOverride);
    if ((!prompt && (!attachments || attachments.length === 0)) || currentRunRef.current || viewedLive().runStatus?.phase === 'saving_results' || sessionOperationRef.current || !currentSettings || !effective) {
      if (sessionOperationRef.current) setLastError('Wait for the current session operation to finish.');
      else if (!effective) setLastError('Configure a provider and model before sending a message.');
      return false;
    }
    const imageAttachments = attachments?.filter((attachment) =>
      !attachment.isDir && /\.(png|jpe?g|webp|gif)$/i.test(attachment.path));
    const hasImages = Boolean(imageAttachments?.length)
      || dataRef.current.messages.some((message) => Boolean(message.imageInputs?.length || message.hasImageInputs));
    const providers = options?.council && options.candidateA && options.candidateB
      ? [options.candidateA.provider, options.candidateB.provider]
      : [effective.provider];
    // Only reject known text-only model IDs; other providers validate their own capabilities.
    const textOnly = hasImages && providers.find((provider) =>
      /^(deepseek-chat|deepseek-reasoner)$/i.test(provider.model.trim()));
    if (textOnly) {
      setLastError(`${textOnly.model} does not support images. Choose a vision-capable model for this conversation.`);
      return false;
    }
    // Only reuse bytes belonging to files still selected in this draft. Copy the
    // payload now so caller-owned options cannot mutate a pending preparation.
    const suppliedImageInputs = options?.imageInputs?.filter((input) =>
      imageAttachments?.some((attachment) => attachment.path === input.sourcePath))
      .map((input) => ({ ...input })) ?? [];
    let imageInputs = suppliedImageInputs.length ? suppliedImageInputs : undefined;
    const missingImageAttachments = imageAttachments?.filter((attachment) =>
      !suppliedImageInputs.some((input) => input.sourcePath === attachment.path));
    if (missingImageAttachments?.length) {
      const sessionId = sessionIdRef.current;
      const navigation = navigationRef.current;
      const modelOverride = dataRef.current.modelOverride;
      const settingsRevision = settingsRevisionRef.current;
      const isOrigin = () => sessionIdRef.current === sessionId && navigationRef.current === navigation;
      const isModelOrigin = () => settingsRef.current === currentSettings
        && settingsRevisionRef.current === settingsRevision && dataRef.current.modelOverride === modelOverride;
      const operation = Symbol('prepareImages');
      const preparing = viewedLive();
      const preparation: RunStatus = { runId: null, sessionId, origin: options?.council ? 'council' : 'chat', phase: 'preparing_attachments', startedAt: Date.now(), phaseSince: Date.now() };
      preparing.runStatus = preparation;
      publishLive(preparing);
      let preparedOk = false;
      sessionOperationRef.current = operation;
      setLastError(null);
      try {
        const prepared = await invoke<import('../types/agent').ImageInput[]>('prepare_image_attachments', { attachments: missingImageAttachments });
        imageInputs = [...suppliedImageInputs, ...prepared];
        // Navigation (including clear in the same session) invalidates pending attachment work.
        if (!isOrigin()) return false;
        if (!isModelOrigin()) {
          setLastError('The selected model or run settings changed while preparing images. Send again with the current model.');
          return false;
        }
        if (currentRunRef.current || sessionOperationRef.current !== operation) return false;
        preparedOk = true;
      } catch (error) {
        if (sessionOperationRef.current === operation && isOrigin() && isModelOrigin()) setLastError(`Could not prepare image attachments: ${String(error)}`);
        return false;
      } finally {
        if (sessionOperationRef.current === operation) sessionOperationRef.current = false;
        if (!preparedOk && preparing.runStatus === preparation) {
          preparing.runStatus = { ...preparation, phase: 'error', phaseSince: Date.now(), endedAt: Date.now() };
          publishLive(preparing);
        }
      }
    }
    const live = viewedLive();
    const originWorkspace = live.data.session.workspaceCwd ?? workspaceRef.current;
    setLastError(null);
    const runId = makeId('run');
    const userMessage: ChatMessage = {
      id: makeId('msg'),
      role: 'user',
      content: prompt ? rawPrompt : '',
      attachments,
      imageInputs,
      ...(selectedSkills === undefined ? {} : { selectedSkills }),
      timestamp: Date.now(),
    };
    const assistantId = `msg_assistant_${runId}`;
    const assistantMessage: ChatMessage = {
      id: assistantId,
      role: 'assistant',
      content: '',
      timestamp: Date.now(),
      toolCalls: [],
      segments: [{ id: 'seg_0', text: '', toolCalls: [] }],
      runId,
      isStreaming: true,
      councilTurn: options?.council && options.candidateA && options.candidateB ? {
        candidateA: {
          providerId: options.candidateA.provider.id,
          model: options.candidateA.provider.model,
          content: '',
          reasoning: '',
          isStreaming: true,
        },
        candidateB: {
          providerId: options.candidateB.provider.id,
          model: options.candidateB.provider.model,
          content: '',
          reasoning: '',
          isStreaming: true,
        },
      } : undefined,
    };
    let activePrompt = userMessage.content;
    if (/^\/plan(\s+.*)?$/i.test(prompt)) {
      const task = prompt.replace(/^\/plan\s*/i, '').trim();
      activePrompt = `Please formulate a structured step-by-step execution plan for: ${task || 'the current task'}.\n\nFormat your plan inside a \`\`\`plan code block with checkable steps:\n\`\`\`plan\n- [ ] Step 1: <Description>\n- [ ] Step 2: <Description>\n- [ ] Step 3: <Description>\n\`\`\`\n\nThen proceed to carry out the steps sequentially, updating and reporting progress as each step completes.`;
    } else if (/^\/goal(\s+.*)?$/i.test(prompt)) {
      const target = prompt.replace(/^\/goal\s*/i, '').trim();
      activePrompt = `You are running in autonomous /goal mode. Target goal: ${target || 'Implement and verify the current task'}.\n\nRules for this mode:\n1. Formulate the verification criteria (test command or build command).\n2. Execute implementation steps.\n3. Run the verification test/build commands to inspect real results.\n4. If failures occur, diagnose and repair root causes without asking for permission, looping until all checks pass.\n5. Only report completion once verification has passed 100%.`;
    } else if (/^\/grill-me(\s+.*)?$/i.test(prompt)) {
      const topic = prompt.replace(/^\/grill-me\s*/i, '').trim();
      activePrompt = `You are running in reverse-interview mode (/grill-me). Topic: ${topic || 'the upcoming task and architecture'}.\n\nRules for this mode:\n1. Do not start writing code or implementing yet.\n2. Proactively grill and interview me with sharp, critical questions about requirements, edge cases, error modes, dependencies, and constraints.\n3. Ask 1-3 targeted questions at a time using the ask_question tool (or clear numbered options) so I can decide directly.\n4. Challenge unstated assumptions and probe potential failure modes.\n5. Once all edge cases and requirements are clarified and agreed upon, provide a bulleted summary of our consensus.`;
    } else if (/^\/(search|web)(\s+.*)?$/i.test(prompt)) {
      const query = prompt.replace(/^\/(search|web)\s*/i, '').trim();
      activePrompt = `Please search the web for "${query || 'latest documentation and updates'}", extract and analyze the top authoritative results using the search_web and fetch_web_page tools, and provide a clear, comprehensive answer with cited source links.`;
    }

    let formattedPrompt = activePrompt;
    if (attachments && attachments.length > 0) {
      const list = attachments
        .map((a) => `- ${a.isDir ? '[Directory]' : '[File]'} \`${a.path}\``)
        .join('\n');
      formattedPrompt = activePrompt
        ? `${activePrompt}\n\n## Attached context:\n${list}`
        : `Please examine and help with the following attached context:\n${list}`;
    }
    const apiUserMessage: ChatMessage = {
      ...userMessage,
      content: formattedPrompt,
    };
    const requestWindow = dataRef.current.historyWindow;
    const requestMessages = [...dataRef.current.messages, apiUserMessage];
    const firstMessage = !(dataRef.current.historyWindow?.offset) && !dataRef.current.messages.some((message) => message.role === 'user');
    mutateData((current) => ({
      ...current,
      session: {
        ...current.session,
        title: (firstMessage && !manualRenamesRef.current.has(current.session.id))
          ? (prompt.split(/\s+/).join(' ').slice(0, 70) || (attachments?.[0]?.name ? `Context: ${attachments[0].name}` : 'New Chat'))
          : current.session.title,
        updatedAt: Date.now(),
      },
      messages: [...dataRef.current.messages, userMessage, assistantMessage],
    }));
    live.checkpointGeneration += 1;
    live.checkpointSequence = 0;
    live.checkpointArtifactIds.clear();
    live.journalSnapshot = undefined;
    live.nativeCheckpointRevision = undefined;
    live.cancellationPending = false;
    live.runId = runId;
    live.stopRequested = false;
    live.failed = false;
    live.startedAt = Date.now();
    live.runStatus = { sessionId: live.data.session.id, runId, origin: options?.council ? 'council' : 'chat', phase: 'saving_results', startedAt: live.startedAt, phaseSince: live.startedAt };
    live.messageId = assistantId;
    live.cancelledBeforeStart = false;
    live.starting = true;
    live.subagents = [];
    live.skills = [];
    publishLive(live);
    setIsStreaming(true);
    setStopRequested(false);
    setSubagents([]);
    setActiveSkills([]);

    const selectedExecutionMode: ExecutionMode = options?.executionMode ?? currentSettings.executionMode ?? 'turbo';

    if (options?.council && options.candidateA && options.candidateB) {
      const requestA: AgentRunRequest = {
        runId,
        sessionId: live.data.session.id,
        workspaceCwd: originWorkspace,
        provider: options.candidateA.provider,
        systemPrompt: currentSettings.systemPrompt || undefined,
        messages: requestMessages,
        thinking: options.candidateA.thinking ?? effective.thinking,
        reasoningEffort: options.candidateA.reasoningEffort ?? effective.reasoningEffort,
        temperature: currentSettings.temperature,
        maxTokens: currentSettings.maxTokens,
        maxTurns: 1,
        streamResponse: true,
        skillPaths: currentSettings.skillPaths,
        mcpServers: [],
        self_improvement_enabled: false,
        reviewConfig: currentSettings.reviewConfig,
        autoCompactionEnabled: currentSettings.autoCompactionEnabled ?? true,
        autoCompactionThresholdTokens: currentSettings.autoCompactionThresholdTokens ?? 65536,
        executionMode: selectedExecutionMode,
      };
      const requestB: AgentRunRequest = {
        runId,
        sessionId: live.data.session.id,
        workspaceCwd: originWorkspace,
        provider: options.candidateB.provider,
        systemPrompt: currentSettings.systemPrompt || undefined,
        messages: requestMessages,
        thinking: options.candidateB.thinking ?? effective.thinking,
        reasoningEffort: options.candidateB.reasoningEffort ?? effective.reasoningEffort,
        temperature: currentSettings.temperature,
        maxTokens: currentSettings.maxTokens,
        maxTurns: 1,
        streamResponse: true,
        skillPaths: currentSettings.skillPaths,
        mcpServers: [],
        self_improvement_enabled: false,
        reviewConfig: currentSettings.reviewConfig,
        autoCompactionEnabled: currentSettings.autoCompactionEnabled ?? true,
        autoCompactionThresholdTokens: currentSettings.autoCompactionThresholdTokens ?? 65536,
        executionMode: selectedExecutionMode,
      };
      try {
        await persistData(live.data);
        if (live.cancelledBeforeStart || live.runId !== runId) return true;
        live.starting = false;
        transitionRun(live, 'waiting_provider');
        publishLive(live);
        requestA.historyWindow = requestWindow ? { ...requestWindow, revision: live.nativeWindow?.revision ?? requestWindow.revision } : undefined;
        requestB.historyWindow = requestA.historyWindow;
        await invoke('send_council_message', { requestA, requestB });
      } catch (error) {
        if (live.runId !== runId) return true;
        if (sessionIdRef.current === live.data.session.id) setLastError(`Failed to start council agent: ${String(error)}`);
        mutateData((current) => ({
          ...current,
          messages: current.messages.map((message) => message.id === assistantId
            ? { ...message, content: `Error: ${String(error)}`, isStreaming: false, runStatus: 'failed' }
            : message),
        }), live);
        live.runId = null;
        live.messageId = null;
        finishRun(live, {answer: 'error', filesChanged: undefined, verification: 'unknown'});
      } finally {
        if (live.runStatus?.runId === runId) live.starting = false;
      }
      return true;
    }

    const request: AgentRunRequest = {
      runId,
      sessionId: live.data.session.id,
        workspaceCwd: originWorkspace,
      provider: effective.provider,
      systemPrompt: currentSettings.systemPrompt || undefined,
      messages: requestMessages,
      thinking: effective.thinking,
      reasoningEffort: effective.reasoningEffort,
      temperature: currentSettings.temperature,
      maxTokens: currentSettings.maxTokens,
      maxTurns: currentSettings.maxTurns,
      streamResponse: currentSettings.streamResponse,
      skillPaths: currentSettings.skillPaths,
      mcpServers: currentSettings.mcpServers,
      self_improvement_enabled: currentSettings.self_improvement_enabled === true,
      reviewConfig: currentSettings.reviewConfig,
      autoCompactionEnabled: currentSettings.autoCompactionEnabled ?? true,
      autoCompactionThresholdTokens: currentSettings.autoCompactionThresholdTokens ?? 65536,
      executionMode: selectedExecutionMode,
    };
    try {
      await persistData(live.data);
      if (live.cancelledBeforeStart || live.runId !== runId) return true;
      live.starting = false;
      transitionRun(live, 'waiting_provider');
      publishLive(live);
      request.historyWindow = requestWindow ? { ...requestWindow, revision: live.nativeWindow?.revision ?? requestWindow.revision } : undefined;
      await invoke('send_message', { request });
    } catch (error) {
      if (live.runId !== runId) return true;
      if (sessionIdRef.current === live.data.session.id) setLastError(`Failed to start agent: ${String(error)}`);
      mutateData((current) => ({
        ...current,
        messages: current.messages.map((message) => message.id === assistantId
          ? { ...message, content: message.content || `Error: ${String(error)}`, isStreaming: false, runStatus: 'failed' }
          : message),
      }), live);
      live.runId = null;
      live.messageId = null;
      finishRun(live, {answer: 'error', filesChanged: undefined, verification: 'unknown'});
    } finally {
      if (live.runStatus?.runId === runId) live.starting = false;
    }
    return true;
  }, [mutateData, persistData, reviewSession, publishLive, viewedLive, transitionRun, finishRun]);

  const stopAgent = useCallback(async () => {
    const live = viewedLive();
    const runId = live.runId;
    if (live.runStatus?.phase === 'preparing_attachments') {
      sessionOperationRef.current = false;
      live.runStatus = { ...live.runStatus, phase: 'cancelled', phaseSince: Date.now(), endedAt: Date.now() };
      publishLive(live);
      return;
    }
    if (!runId || live.stopRequested) return;
    live.stopRequested = true;
    live.cancellationPending = true;
    // Keep the last real sidecar progress until native completion enriches tools.
    if (live.checkpointTimer !== null) { clearTimeout(live.checkpointTimer); live.checkpointTimer = null; }
    if (live.starting) {
      live.cancelledBeforeStart = true;
      live.runId = null;
      live.messageId = null;
      live.starting = false;
    }
    publishLive(live);
    setStopRequested(live.stopRequested);
    setSubagents((prev) =>
      prev.map((s) =>
        s.status === 'running'
          ? { ...s, status: 'cancelled', completedAt: s.completedAt || Date.now() }
          : s
      )
    );
    setActiveSkills([]);
    mutateData((current) => ({
      ...current,
      messages: current.messages.map((message) =>
        message.id === streamingMessageIdRef.current || (message.role === 'assistant' && message.runId === runId)
          ? cancelledMessage(message)
          : message
      ),
    }));
    if (live.cancelledBeforeStart) {
      live.stopRequested = false;
      finishRun(live, {answer: 'cancelled', filesChanged: false, verification: 'not_run'});
      return;
    }
    try {
      await invoke('abort_agent', { runId });
    } catch (error) {
      setLastError(`Could not stop run: ${String(error)}`);
      live.stopRequested = false;
      publishLive(live);
    }

  }, [mutateData, persistData, publishLive, viewedLive, finishRun]);

  const detachCurrentRun = useCallback(() => {
    const live = viewedLive();
    // Detached/cleared turns no longer own the visible runtime status.
    // Also invalidates any outstanding final-save status acknowledgement.
    live.runStatus = null;
    flushPending(live);
    const runId = live.runId;
    live.cancelledBeforeStart = true;
    live.checkpointGeneration += 1;
    if (live.checkpointTimer !== null) { clearTimeout(live.checkpointTimer); live.checkpointTimer = null; }
    live.subagents = live.subagents.map((sub) => sub.status === 'running' ? { ...sub, status: 'cancelled', completedAt: Date.now() } : sub);
    live.skills = [];
    live.data = { ...live.data, messages: live.data.messages.map((message) => message.runId === runId ? cancelledMessage(message) : message) };
    live.dirty = true;
    live.runId = null;
    live.messageId = null;
    live.stopRequested = false;
    publishLive(live);
    if (runId) void invoke('abort_agent', { runId }).catch((error) => setLastError(`Could not stop run: ${String(error)}`));
  }, [flushPending, publishLive, viewedLive]);

  const resetSession = useCallback((persistPrevious: boolean): string => {
    navigationRef.current += 1;
    navigationPendingRef.current = false;
    const previous = viewedLive();
    flushPending(previous);
    if (persistPrevious && previous.dirty) void persistData(previous.data).catch(() => undefined);
    const next = liveSession(createSession());
    liveSessionsRef.current.set(next.data.session.id, next);
    sessionIdRef.current = next.data.session.id;
    publishLive(next);
    pruneCache();
    setIsSessionLoading(false);
    setWorkspaceCwd('');
    workspaceRef.current = '';
    void invoke('set_workspace_cwd', { newCwd: '' }).catch((error) => setLastError(`Could not clear workspace: ${String(error)}`));
    setLastError(null);
    return next.data.session.id;
  }, [flushPending, persistData, publishLive, viewedLive, pruneCache]);

  const newSession = useCallback(() => { preview.clear(); return resetSession(true); }, [resetSession, preview.clear]);

  const switchSession = useCallback(async (sessionId: string) => {
    preview.clear();
    const started = performance.now();
    const navigation = ++navigationRef.current;
    if (sessionId === sessionIdRef.current) {
      navigationPendingRef.current = false;
      setIsSessionLoading(false);
      setLastError(null);
      return;
    }
    navigationPendingRef.current = true;
    setIsSessionLoading(true);
    const previous = viewedLive();
    flushPending(previous);
    try {
      if (previous.dirty) void persistData(previous.data).catch(() => undefined);
      let next = liveSessionsRef.current.get(sessionId);
      const kind = next ? 'cache' as const : 'cold' as const;
      if (!next) {
        const page = await invoke<SessionPage>('load_session_page', { sessionId, limit: 50 });
        next = liveSessionsRef.current.get(sessionId) ?? liveSession(restoreSession({ ...page.data, historyWindow: page.window }, reviewResultsRef.current));
      }
      if (!next) return;
      if (navigationRef.current !== navigation) return;
      liveSessionsRef.current.delete(sessionId);
      liveSessionsRef.current.set(sessionId, next);
      sessionIdRef.current = sessionId;
      flushPending(next);
      publishLive(next);
      recordSessionLoad({ kind, durationMs: performance.now() - started, loaded: next.data.messages.length, total: next.data.historyWindow?.total ?? next.data.messages.length, at: Date.now() });
      pruneCache();
      setLastError(null);
      const sessCwd = next.data.session.workspaceCwd || '';
      workspaceRef.current = sessCwd;
      setWorkspaceCwd(sessCwd);
      await invoke('set_workspace_cwd', { newCwd: sessCwd });
    } catch (error) {
      if (navigationRef.current === navigation) setLastError(`Could not load session: ${String(error)}`);
    } finally {
      if (navigationRef.current === navigation) { navigationPendingRef.current = false; setIsSessionLoading(false); }
    }
  }, [flushPending, persistData, publishLive, viewedLive, pruneCache]);

  const openMessage = useCallback(async (sessionId: string, messageId: string) => {
    const switching = switchSession(sessionId);
    const isPreviewRequest = preview.begin();
    const navigation = navigationRef.current;
    await switching;
    if (sessionIdRef.current !== sessionId || navigationPendingRef.current || navigationRef.current !== navigation || !isPreviewRequest()) return false;
    return preview.open(sessionId, messageId, () => sessionIdRef.current === sessionId && navigationRef.current === navigation);
  }, [switchSession, preview.open, preview.begin]);
  const openSearchResult = useCallback(async (result: import('../types/agent').DeepSearchResult) => {
    if (result.matchMessageId) return openMessage(result.sessionId, result.matchMessageId);
    const switching = switchSession(result.sessionId);
    const navigation = navigationRef.current;
    await switching;
    return sessionIdRef.current === result.sessionId && navigationRef.current === navigation && !navigationPendingRef.current;
  }, [openMessage, switchSession]);
  const loadLatestMessages = preview.clear;

  const clearCurrentSession = useCallback(() => {
    if (preview.isActive()) return;
    sessionOperationRef.current = false;
    navigationRef.current += 1;
    navigationPendingRef.current = false;
    detachCurrentRun();
    const live = viewedLive();
    live.historyError = null;
    live.loadingOlder = false;
    setIsSessionLoading(false);
    mutateData((current) => ({
      ...current,
      historyWindow: undefined,
      session: { ...current.session, title: 'New Chat', messageCount: 0, preview: undefined },
      messages: [],
      artifacts: [],
    }));
    selectArtifact(null);
    setSubagents([]);
    setActiveSkills([]);
    void persistData(dataRef.current).catch(() => undefined);
  }, [mutateData, persistData, selectArtifact, detachCurrentRun, viewedLive]);

  const updateSettings = useCallback(async (next: AgentSettings) => {
    const revision = ++settingsRevisionRef.current;
    settingsRef.current = next;
    setSettings(next);
    const queued = settingsQueueRef.current.then(async () => {
      try {
        await invoke('save_settings', { settings: next });
        confirmedSettingsRef.current = next;
      } catch (error) {
        if (revision === settingsRevisionRef.current) {
          settingsRef.current = confirmedSettingsRef.current;
          setSettings(confirmedSettingsRef.current);
          setLastError(`Could not save settings: ${String(error)}`);
        }
        throw error;
      }
    });
    settingsQueueRef.current = queued.catch(() => undefined);
    await queued;
  }, []);

  const selectModel = useCallback((providerId: string, model: string) => {
    mutateData((current) => ({
      ...current,
      modelOverride: { ...current.modelOverride, providerId, model },
    }));
    if (dataRef.current.messages.length > 0) void persistData(dataRef.current).catch(() => undefined);
  }, [mutateData, persistData]);

  const updateRunConfig = useCallback((override: Partial<SessionModelOverride>) => {
    mutateData((current) => ({
      ...current,
      modelOverride: { ...current.modelOverride, ...override },
    }));
    if (dataRef.current.messages.length > 0) void persistData(dataRef.current).catch(() => undefined);
  }, [mutateData, persistData]);

  const clearModelOverride = useCallback(() => {
    mutateData((current) => ({ ...current, modelOverride: null }));
    if (dataRef.current.messages.length > 0) void persistData(dataRef.current).catch(() => undefined);
  }, [mutateData, persistData]);

  const toggleFavorite = useCallback(async (key: string) => {
    const current = settingsRef.current;
    if (!current) return;
    const favorites = current.favoriteModels.includes(key)
      ? current.favoriteModels.filter((item) => item !== key)
      : [...current.favoriteModels, key];
    await updateSettings({ ...current, favoriteModels: favorites });
  }, [updateSettings]);

  const addCustomModel = useCallback(async (providerId: string, model: string) => {
    const origin = sessionIdRef.current;
    const navigation = navigationRef.current;
    const current = settingsRef.current;
    const clean = (model || '').trim();
    if (!current || !clean) return;
    const next: AgentSettings = {
      ...current,
      providers: current.providers.map((provider) => provider.id === providerId
        ? { ...provider, customModels: [...new Set([...(provider.customModels || []), clean])] }
        : provider),
    };
    await updateSettings(next);
    if (sessionIdRef.current !== origin || navigationRef.current !== navigation) return;
    selectModel(providerId, clean);
  }, [selectModel, updateSettings]);

  const refreshModels = useCallback(async (providerId: string) => {
    const current = settingsRef.current;
    const provider = current?.providers.find((item) => item.id === providerId);
    if (!current || !provider) throw new Error('Provider not found');
    const models = await invoke<string[]>('fetch_provider_models', { provider });
    const latest = settingsRef.current;
    const latestProvider = latest?.providers.find((item) => item.id === providerId);
    if (!latest || !latestProvider) return models;
    // A response from the old endpoint must not overwrite an edited provider.
    if (latestProvider.baseUrl !== provider.baseUrl || latestProvider.apiKey !== provider.apiKey || latestProvider.kind !== provider.kind) return models;
    const next: AgentSettings = {
      ...latest,
      providers: latest.providers.map((item) => item.id === providerId ? { ...item, models } : item),
    };
    await updateSettings(next);
    return models;
  }, [updateSettings]);

  const renameSession = useCallback(async (sessionId: string, title: string) => {
    manualRenamesRef.current.add(sessionId);
    const navigation = navigationRef.current;
    try {
      const live = liveSessionsRef.current.get(sessionId);
      if (live?.runId) flushPending(live);
      const prepared = live && (live.dirty || live.journalSnapshot || live.deltaBusy) ? persistData(live.data) : persistenceQueueRef.current;
      const queued = prepared.then(async () => {
        const origin = liveSessionsRef.current.get(sessionId);
        const baseline = origin?.committedData;
        const renamed = await invoke<Session>('rename_session_summary', { sessionId, title });
        if (origin) mutateData((current) => ({ ...current, session: { ...current.session, title: renamed.title } }), origin);
        if (origin && baseline) await refreshNativeWindow(origin, baseline, true);
        await refreshSessions();
      });
      persistenceQueueRef.current = queued.catch(() => undefined);
      await queued;
    } catch (error) {
      if (navigationRef.current === navigation) setLastError('Could not rename session: ' + String(error));
    }
  }, [mutateData, persistData, refreshSessions, refreshNativeWindow, flushPending]);

  const generateSessionTitle = useCallback(async (sessionId?: string) => {
    const sid = sessionId || sessionIdRef.current;
    if (!sid) return null;
    const targetData = sid === sessionIdRef.current ? dataRef.current : liveSessionsRef.current.get(sid)?.data;
    if (!targetData) return null;
    const firstUser = targetData.messages.find((m) => m.role === 'user');
    const firstAssistant = targetData.messages.find(
      (m) => m.role === 'assistant' && (m.content || m.preamble || '').trim()
    );
    if (!firstUser) return null;
    const promptText = (firstUser.content || '').trim();
    const replySnippet = (firstAssistant?.content || firstAssistant?.preamble || '').trim().slice(0, 300);
    const provider = resolveEffective(settingsRef.current, targetData.modelOverride)?.provider;
    if (!provider || !promptText) return null;

    try {
      const newTitle = await invoke<string>('generate_session_title', {
        sessionId: sid,
        provider,
        prompt: promptText,
        responseSnippet: replySnippet || undefined,
      });
      if (newTitle && typeof newTitle === 'string' && newTitle.trim()) {
        const cleanTitle = newTitle.trim();
        const origin = liveSessionsRef.current.get(sid);
        if (origin) {
          mutateData((current) => ({ ...current, session: { ...current.session, title: cleanTitle } }), origin);
        } else if (sid === sessionIdRef.current) {
          mutateData((current) => ({ ...current, session: { ...current.session, title: cleanTitle } }));
        }
        await refreshSessions();
        return cleanTitle;
      }
    } catch {
      return null;
    }
    return null;
  }, [mutateData, refreshSessions]);

  const togglePinSession = useCallback(async (sessionId: string) => {
    const navigation = navigationRef.current;
    try {
      const live = liveSessionsRef.current.get(sessionId);
      if (live?.runId) flushPending(live);
      const prepared = live && (live.dirty || live.journalSnapshot || live.deltaBusy) ? persistData(live.data) : persistenceQueueRef.current;
      const queued = prepared.then(async () => {
        const origin = liveSessionsRef.current.get(sessionId);
        const baseline = origin?.committedData;
        const updated = await invoke<Session>('toggle_pin_session', { sessionId });
        if (origin) mutateData((current) => ({
          ...current,
          session: { ...current.session, pinned: updated.pinned, pinnedAt: updated.pinnedAt },
        }), origin);
        if (origin && baseline) await refreshNativeWindow(origin, baseline, true);
        await refreshSessions();
      });
      persistenceQueueRef.current = queued.catch(() => undefined);
      await queued;
    } catch (error) {
      if (navigationRef.current === navigation) setLastError('Could not toggle pin on session: ' + String(error));
    }
  }, [mutateData, refreshSessions, refreshNativeWindow, flushPending, persistData]);

  const deleteSession = useCallback(async (sessionId: string) => {
    if (sessionId === sessionIdRef.current) { detachCurrentRun(); resetSession(false); }
    else {
      const live = liveSessionsRef.current.get(sessionId);
      if (live) {
        live.cancelledBeforeStart = true;
        if (live.runId) void invoke('abort_agent', { runId: live.runId }).catch(() => undefined);
        if (live.frame !== null) cancelAnimationFrame(live.frame);
        live.runId = null;
        live.messageId = null;
      }
    }
    liveSessionsRef.current.delete(sessionId);
    const activities = { ...sessionActivitiesRef.current };
    delete activities[sessionId];
    sessionActivitiesRef.current = activities;
    setSessionActivities(activities);
    try {
      await persistenceQueueRef.current;
      await invoke('delete_session', { sessionId });
      await refreshSessions();
    } catch (error) {
      setLastError(`Could not delete session: ${String(error)}`);
    }
  }, [refreshSessions, resetSession]);

  const exportSession = useCallback(async (
    sessionId: string,
    format: 'json' | 'markdown' | 'html',
    requestedDestination?: string,
  ) => {
    if (sessionId === sessionIdRef.current && currentRunRef.current) {
      setLastError('Stop the active run before exporting this session.');
      return false;
    }
    try {
      const session = sessions.find((item) => item.id === sessionId) || dataRef.current.session;
      const extension = format === 'json' ? 'json' : format === 'html' ? 'html' : 'md';
      const filterLabel = format === 'json' ? 'JSON' : format === 'html' ? 'HTML Document' : 'Markdown';
      let destination = (requestedDestination || '').trim() || null;
      if (!destination) {
        destination = await save({
          defaultPath: `${session.title.replace(/[<>:"/\\|?*]/g, '_').slice(0, 80) || 'aether-session'}.${extension}`,
          filters: [{ name: filterLabel, extensions: [extension] }],
        });
      }
      if (!destination) return false;
      if (sessionId === sessionIdRef.current) {
        flushPending();
        await persistData(dataRef.current);
      } else {
        await persistenceQueueRef.current;
      }
      await invoke('export_session', { sessionId, destination, format });
      return true;
    } catch (error) {
      setLastError(`Could not export session: ${String(error)}`);
      return false;
    }
  }, [sessions, flushPending, persistData]);

  const selectWorkspace = useCallback(async () => {
    const origin = viewedLive();
    const navigation = navigationRef.current;
    const isOrigin = () => navigationRef.current === navigation && sessionIdRef.current === origin.data.session.id;
    try {
      const selected = await open({ directory: true, multiple: false, defaultPath: workspaceRef.current || undefined });
      if (typeof selected !== 'string' || !isOrigin()) return;
      const cwd = await invoke<string>('set_workspace_cwd', { newCwd: selected });
      mutateData((current) => ({ ...current, session: { ...current.session, workspaceCwd: cwd } }), origin);
      if (isOrigin()) {
        workspaceRef.current = cwd;
        setWorkspaceCwd(cwd);
      }
      await persistData(origin.data);
    } catch (error) {
      if (isOrigin()) setLastError(`Could not switch workspace: ${String(error)}`);
    }
  }, [mutateData, persistData, viewedLive]);

  const clearWorkspace = useCallback(async () => {
    const origin = viewedLive();
    const navigation = navigationRef.current;
    const isOrigin = () => navigationRef.current === navigation && sessionIdRef.current === origin.data.session.id;
    try {
      await invoke('set_workspace_cwd', { newCwd: '' });
      mutateData((current) => ({
        ...current,
        session: { ...current.session, workspaceCwd: undefined },
      }), origin);
      if (isOrigin()) {
        workspaceRef.current = '';
        setWorkspaceCwd('');
      }
      await persistData(origin.data);
    } catch (error) {
      if (isOrigin()) setLastError(`Could not clear workspace: ${String(error)}`);
    }
  }, [mutateData, persistData, viewedLive]);

  const listWorkspaceFiles = useCallback(async (query: string, limit = 30) => {
    return invoke<string[]>('list_workspace_files', { query, limit });
  }, []);

  const listSkills = useCallback(async (paths?: string[]) => {
    return invoke<import('../types/agent').SkillInfo[]>('list_skills', { paths,
      expectedWorkspace: dataRef.current.session.workspaceCwd ?? workspaceRef.current });
  }, []);

  const listAllArtifacts = useCallback(async (): Promise<import('../types/agent').GlobalArtifactItem[]> => {
    try {
      return await invoke<import('../types/agent').GlobalArtifactItem[]>('list_all_artifacts');
    } catch (error) {
      setLastError(`Could not load artifacts library: ${String(error)}`);
      throw error;
    }
  }, []);

  const deepSearch = useCallback(async (query: string, limit = 20): Promise<import('../types/agent').DeepSearchResult[]> => {
    try {
      return await invoke<import('../types/agent').DeepSearchResult[]>('deep_search', { query, limit });
    } catch (error) {
      setLastError(`Could not search sessions: ${String(error)}`);
      throw error;
    }
  }, []);

  const refreshGitStatus = useCallback(async (): Promise<GitRepoStatus | null> => {
    try {
      const status = await invoke<GitRepoStatus>('git_status');
      setGitStatus(status);
      return status;
    } catch {
      setGitStatus(null);
      return null;
    }
  }, []);

  const commitWorkspaceChanges = useCallback(async (message: string): Promise<string> => {
    try {
      const res = await invoke<string>('git_commit', { message });
      await refreshGitStatus();
      return res;
    } catch (error) {
      const err = String(error);
      setLastError(`Git commit failed: ${err}`);
      throw new Error(err);
    }
  }, [refreshGitStatus]);

  useEffect(() => {
    if (workspaceCwd) {
      void refreshGitStatus();
    } else {
      setGitStatus(null);
    }
  }, [workspaceCwd, refreshGitStatus]);

  useEffect(() => {
    setBackgroundTasks([]);
    if (!isLoading) void refreshBackgroundTasks();
  }, [data.session.id, isLoading, refreshBackgroundTasks]);

  useEffect(() => {
    const hasRunning = backgroundTasks.some((t) => t.status === 'running');
    if (!hasRunning) return;
    const interval = setInterval(() => {
      void refreshBackgroundTasks();
    }, 2000);
    return () => clearInterval(interval);
  }, [backgroundTasks, refreshBackgroundTasks]);

  const activeArtifact = useMemo(
    () => data.artifacts.find((artifact) => artifact.id === activeArtifactId) || null,
    [data.artifacts, activeArtifactId],
  );

  const selectCouncilWinner = useCallback((messageId: string, winner: 'A' | 'B') => {
    const live = viewedLive();
    const message = live.data.messages.find((item) => item.id === messageId);
    if (!message?.councilTurn || live.runId || message.isStreaming
      || message.councilTurn.candidateA.isStreaming || message.councilTurn.candidateB.isStreaming) return;
    flushPending(live);
    mutateData((current) => ({
      ...current,
      messages: current.messages.map((m) => {
        if (m.id !== messageId || !m.councilTurn) return m;
        const winnerCandidate = winner === 'A' ? m.councilTurn.candidateA : m.councilTurn.candidateB;
        return {
          ...m,
          content: winnerCandidate.content,
          councilTurn: {
            ...m.councilTurn,
            chosenWinner: winner,
          },
        };
      }),
    }), live);
    void persistData(live.data).catch(() => undefined);
  }, [flushPending, mutateData, persistData, viewedLive]);

  const compactSession = useCallback(async () => {
    if (currentRunRef.current || sessionOperationRef.current) {
      setLastError('Stop the active run and wait for session operations before compacting.');
      return;
    }
    if (!dataRef.current.session.id || dataRef.current.messages.length < 3) {
      setLastError('Conversation requires at least 3 messages to compact.');
      return;
    }
    const origin = viewedLive();
    const sessionId = origin.data.session.id;
    const navigation = navigationRef.current;
    const operation = Symbol('compactSession');
    sessionOperationRef.current = operation;
    origin.persistenceBusy += 1;
    setLastError(null);
    setIsLoading(true);
    const snapshot = origin.data;
    const queued = persistData(snapshot).then(async () => {
      if (navigationRef.current !== navigation || sessionIdRef.current !== sessionId) return;
      const updated = await invoke<SessionData>('compact_session', { sessionId });
      origin.historyEpoch += 1;
      const normalized: SessionData = {
        ...updated,
        historyWindow: undefined,
        messages: updated.messages.map((message) => ({ ...message, isStreaming: false })),
      };
      // Native already committed this replacement, including when we navigated away.
      origin.data = normalized;
      origin.committedData = normalized;
      origin.nativeWindow = undefined;
      origin.nativeCheckpointRevision = undefined;
      origin.journalSnapshot = undefined;
      origin.authoritative = true;
      origin.dirty = false;
      publishLive(origin);
    });
    persistenceQueueRef.current = queued.catch(() => undefined);
    try {
      await queued;
    } catch (error) {
      if (navigationRef.current === navigation && sessionIdRef.current === sessionId) {
        setLastError(`Context compaction failed: ${String(error)}`);
      }
    } finally {
      // Also show a saved backup if replacing the original failed afterward.
      await refreshSessions();
      origin.persistenceBusy -= 1;
      if (sessionOperationRef.current === operation) {
        sessionOperationRef.current = false;
        setIsLoading(false);
      }
    }
  }, [persistData, viewedLive, publishLive, refreshSessions]);

  const ensureFullHistory = useCallback(async (): Promise<SessionData | null> => {
    const live = viewedLive();
    if (!live.data.historyWindow) return live.data;
    const sessionId = live.data.session.id;
    const navigation = navigationRef.current;
    const isOrigin = () => navigationRef.current === navigation && sessionIdRef.current === sessionId;
    const operation = Symbol('hydrateHistory');
    sessionOperationRef.current = operation;
    setIsSessionLoading(true);
    try {
      flushPending(live);
      if (live.dirty) await persistData(live.data, false, isOrigin);
      await persistenceQueueRef.current;
      if (!isOrigin()) return null;
      const baseline = live.data;
      const full = await invoke<SessionData>('load_session', { sessionId });
      if (!isOrigin()) return null;
      if (live.data !== baseline) throw new Error('Session changed while loading full history. Retry the operation.');
      const restored = restoreSession({ ...full, historyWindow: undefined, session: baseline.session, modelOverride: baseline.modelOverride }, reviewResultsRef.current);
      // Restore legacy fields before comparing so equivalent loaded tail rows
      // retain their original identity (including attachment/image arrays).
      const normalized = restoreSession(baseline, reviewResultsRef.current);
      const existingMessages = new Map(baseline.messages.map((message, index) => [message.id, { message, normalized: normalized.messages[index] }]));
      const messages = restored.messages.map((message) => {
        const existing = existingMessages.get(message.id);
        return existing && JSON.stringify(existing.normalized) === JSON.stringify(message) ? existing.message : message;
      });
      const existingArtifacts = new Map(baseline.artifacts.map((artifact) => [artifact.id, artifact]));
      const artifacts = restored.artifacts.map((artifact) => {
        const existing = existingArtifacts.get(artifact.id);
        return existing && JSON.stringify(existing) === JSON.stringify(artifact) ? existing : artifact;
      });
      live.data = {
        ...restored,
        messages: messages.length === baseline.messages.length && messages.every((message, index) => message === baseline.messages[index]) ? baseline.messages : messages,
        artifacts: artifacts.length === baseline.artifacts.length && artifacts.every((artifact, index) => artifact === baseline.artifacts[index]) ? baseline.artifacts : artifacts,
      };
      live.nativeWindow = undefined;
      live.historyError = null;
      publishLive(live);
      return live.data;
    } catch (error) {
      if (isOrigin()) setLastError('Could not load full session history: ' + String(error));
      return null;
    } finally {
      if (sessionOperationRef.current === operation) sessionOperationRef.current = false;
      if (isOrigin()) setIsSessionLoading(false);
    }
  }, [viewedLive, flushPending, persistData, publishLive]);

  const undoTurn = useCallback(async (): Promise<import('../types/agent').UndoTurnResult | null> => {
    if (currentRunRef.current || sessionOperationRef.current) {
      setLastError('Stop the active run and wait for session operations before undoing a turn.');
      return null;
    }
    let previous = dataRef.current;
    if (previous.historyWindow) {
      const full = await ensureFullHistory();
      if (!full) return null;
      previous = full;
    }
    const sessionId = previous.session.id;
    const navigation = navigationRef.current;
    const isOrigin = () => sessionIdRef.current === sessionId && navigationRef.current === navigation;
    const lastUserIdx = previous.messages.reduce(
      (last, message, index) => (message.role === 'user' ? index : last),
      -1,
    );
    if (lastUserIdx < 0) return null;

    const undone = previous.messages[lastUserIdx];
    const removed = previous.messages.slice(lastUserIdx);
    const messages = previous.messages.slice(0, lastUserIdx);
    const removedArtifacts = new Set(
      removed.flatMap((message) => (message.toolCalls || []).flatMap((tool) => tool.artifactId || [])),
    );
    const remainingArtifacts = new Set(
      messages.flatMap((message) => (message.toolCalls || []).flatMap((tool) => tool.artifactId || [])),
    );
    const artifacts = previous.artifacts.filter((artifact) => {
      if (remainingArtifacts.has(artifact.id)) return true;
      if (removedArtifacts.has(artifact.id)) return false;
      return artifact.timestamp < undone.timestamp;
    });
    const next: SessionData = {
      ...previous,
      session: {
        ...previous.session,
        updatedAt: Date.now(),
        messageCount: messages.length,
        preview: extractPreview(messages),
      },
      messages,
      artifacts,
    };

    const operation = Symbol('undoTurn');
    sessionOperationRef.current = operation;
    setIsLoading(true);
    mutateData(() => next);
    try {
      await persistData(next, false, isOrigin);
      if (!isOrigin()) return null;
      return { content: undone.content, attachments: undone.attachments, imageInputs: undone.imageInputs,
        ...(undone.selectedSkills === undefined ? {} : { selectedSkills: undone.selectedSkills }) };
    } catch {
      if (isOrigin() && !currentRunRef.current) {
        mutateData(() => previous);
      }
      return null;
    } finally {
      if (sessionOperationRef.current === operation) {
        sessionOperationRef.current = false;
        setIsLoading(false);
      }
    }
  }, [ensureFullHistory, mutateData, persistData]);


  const rollbackToMessage = useCallback(async (targetMessageId: string): Promise<boolean> => {
    if (currentRunRef.current || sessionOperationRef.current) {
      setLastError('Stop the active run and wait for session operations before rolling back.');
      return false;
    }
    let previous = dataRef.current;
    if (previous.historyWindow) {
      const full = await ensureFullHistory();
      if (!full) return false;
      previous = full;
    }
    const navigation = navigationRef.current;
    const isOrigin = () => navigationRef.current === navigation && sessionIdRef.current === previous.session.id;
    const targetIdx = previous.messages.findIndex((m) => m.id === targetMessageId);
    if (targetIdx < 0) return false;

    const messages = previous.messages.slice(0, targetIdx + 1);
    const removed = previous.messages.slice(targetIdx + 1);
    if (removed.length === 0) return true;

    const removedArtifacts = new Set(
      removed.flatMap((message) => (message.toolCalls || []).flatMap((tool) => tool.artifactId || [])),
    );
    const remainingArtifacts = new Set(
      messages.flatMap((message) => (message.toolCalls || []).flatMap((tool) => tool.artifactId || [])),
    );
    const targetTimestamp = previous.messages[targetIdx].timestamp;
    const artifacts = previous.artifacts.filter((artifact) => {
      if (remainingArtifacts.has(artifact.id)) return true;
      if (removedArtifacts.has(artifact.id)) return false;
      return artifact.timestamp <= targetTimestamp;
    });

    const next: SessionData = {
      ...previous,
      session: {
        ...previous.session,
        updatedAt: Date.now(),
        messageCount: messages.length,
        preview: extractPreview(messages),
      },
      messages,
      artifacts,
    };

    sessionOperationRef.current = true;
    setIsLoading(true);
    mutateData(() => next);
    try {
      await persistData(next, false, isOrigin);
      return isOrigin();
    } catch (error) {
      if (isOrigin()) setLastError(`Could not rollback session: ${String(error)}`);
      return false;
    } finally {
      sessionOperationRef.current = false;
      setIsLoading(false);
    }
  }, [ensureFullHistory, mutateData, persistData]);

  const editAndResendMessage = useCallback(async (messageId: string, newContent: string): Promise<boolean> => {
    const navigation = navigationRef.current;
    if (currentRunRef.current) detachCurrentRun();
    if (sessionOperationRef.current) {
      setLastError('Wait for session operations before editing.');
      return false;
    }
    const clean = (newContent || '').trim();
    const visibleTarget = dataRef.current.messages.find((message) => message.id === messageId);
    if (visibleTarget && !clean && !visibleTarget.attachments?.length) return false;

    // Capture exact bytes before any destructive save. A page hint is never
    // permission to re-read a possibly deleted or modified attachment file.
    let replayImageInputs = visibleTarget?.imageInputs;
    if (visibleTarget?.hasImageInputs && !replayImageInputs?.length) {
      const sessionId = dataRef.current.session.id;
      const operation = Symbol('hydrateMessageImages');
      sessionOperationRef.current = operation;
      try {
        replayImageInputs = await invoke<import('../types/agent').ImageInput[]>('read_message_image_inputs', { sessionId, messageId });
        if (navigationRef.current !== navigation || sessionIdRef.current !== sessionId) return false;
        if (!replayImageInputs.length) throw new Error('Stored image payload is missing');
      } catch (error) {
        if (navigationRef.current === navigation && sessionIdRef.current === sessionId) setLastError('Could not restore image attachments: ' + String(error));
        return false;
      } finally {
        if (sessionOperationRef.current === operation) sessionOperationRef.current = false;
      }
    }
    let previous = dataRef.current;
    if (previous.historyWindow) {
      const full = await ensureFullHistory();
      if (!full) return false;
      previous = full;
    }
    const targetIdx = previous.messages.findIndex((m) => m.id === messageId);
    if (targetIdx < 0) return false;

    const targetMsg = previous.messages[targetIdx];
    const attachments = targetMsg.attachments;
    if (!clean && !attachments?.length) return false;

    const messages = previous.messages.slice(0, targetIdx);
    const removed = previous.messages.slice(targetIdx);
    const removedArtifacts = new Set(
      removed.flatMap((message) => (message.toolCalls || []).flatMap((tool) => tool.artifactId || [])),
    );
    const remainingArtifacts = new Set(
      messages.flatMap((message) => (message.toolCalls || []).flatMap((tool) => tool.artifactId || [])),
    );
    const artifacts = previous.artifacts.filter((artifact) => {
      if (remainingArtifacts.has(artifact.id)) return true;
      if (removedArtifacts.has(artifact.id)) return false;
      return artifact.timestamp < targetMsg.timestamp;
    });

    const next: SessionData = {
      ...previous,
      session: {
        ...previous.session,
        updatedAt: Date.now(),
        messageCount: messages.length,
        preview: extractPreview(messages),
      },
      messages,
      artifacts,
    };

    sessionOperationRef.current = true;
    mutateData(() => next);
    try {
      await persistData(next);
    } catch (error) {
      setLastError(`Could not prepare turn: ${String(error)}`);
      sessionOperationRef.current = false;
      return false;
    }
    sessionOperationRef.current = false;
    if (navigationRef.current !== navigation || sessionIdRef.current !== previous.session.id) return false;

    const retainedInputs = replayImageInputs ?? targetMsg.imageInputs;
    return sendMessage(newContent, attachments, {
      ...(retainedInputs?.length ? { imageInputs: retainedInputs } : {}),
      ...(targetMsg.selectedSkills === undefined ? {} : { selectedSkills: targetMsg.selectedSkills }),
    });
  }, [ensureFullHistory, mutateData, persistData, sendMessage, detachCurrentRun]);

  const retryTurn = useCallback(async (messageId: string): Promise<boolean> => {
    if (currentRunRef.current) detachCurrentRun();
    if (sessionOperationRef.current) {
      setLastError('Wait for session operations before retrying.');
      return false;
    }
    let previous = dataRef.current;
    if (previous.historyWindow && !previous.messages.slice(0, previous.messages.findIndex((m) => m.id === messageId) + 1).some((m) => m.role === 'user')) {
      const full = await ensureFullHistory();
      if (!full) return false;
      previous = full;
    }
    const targetIdx = previous.messages.findIndex((m) => m.id === messageId);
    if (targetIdx < 0) return false;

    const targetMsg = previous.messages[targetIdx];
    let userMsg: ChatMessage | null = null;

    if (targetMsg.role === 'user') {
      userMsg = targetMsg;
    } else {
      for (let i = targetIdx - 1; i >= 0; i--) {
        if (previous.messages[i].role === 'user') {
          userMsg = previous.messages[i];
          break;
        }
      }
    }

    if (!userMsg) {
      setLastError('No user message found to retry.');
      return false;
    }

    return editAndResendMessage(userMsg.id, userMsg.content || '');
  }, [ensureFullHistory, editAndResendMessage, detachCurrentRun]);

  const initAgentsMd = useCallback(async (expectedWorkspace?: string) => {
    const workspace = expectedWorkspace ?? workspaceRef.current;
    try {
      const result = await invoke<string>('init_workspace_agents_md', { expectedWorkspace: workspace });
      return result;
    } catch (error) {
      setLastError(`Failed to initialize AGENTS.md: ${String(error)}`);
      throw error;
    }
  }, []);

  const readAgentsMd = useCallback(async (expectedWorkspace?: string) => {
    const workspace = expectedWorkspace ?? workspaceRef.current;
    return await invoke<string | null>('read_workspace_agents_md', { expectedWorkspace: workspace });
  }, []);

  const saveAgentsMd = useCallback(async (content: string, expectedWorkspace?: string) => {
    const workspace = expectedWorkspace ?? workspaceRef.current;
    await invoke('save_workspace_agents_md', { content, expectedWorkspace: workspace });
  }, []);

  // Stable viewport guards read the latest live actions without changing row action identities.
  const previewActionsRef = useRef({ reviewSession, sendMessage, selectCouncilWinner, compactSession, undoTurn, rollbackToMessage, editAndResendMessage, retryTurn, loadOlderMessages });
  previewActionsRef.current = { reviewSession, sendMessage, selectCouncilWinner, compactSession, undoTurn, rollbackToMessage, editAndResendMessage, retryTurn, loadOlderMessages };
  const guardedreviewSession = useCallback((...args: Parameters<typeof reviewSession>) => preview.isActive() ? Promise.resolve(false) : previewActionsRef.current.reviewSession(...args), [preview.isActive]);
  const guardedsendMessage = useCallback((...args: Parameters<typeof sendMessage>) => preview.isActive() ? Promise.resolve(false) : previewActionsRef.current.sendMessage(...args), [preview.isActive]);
  const guardedselectCouncilWinner = useCallback((...args: Parameters<typeof selectCouncilWinner>) => preview.isActive() ? undefined : previewActionsRef.current.selectCouncilWinner(...args), [preview.isActive]);
  const guardedcompactSession = useCallback((...args: Parameters<typeof compactSession>) => preview.isActive() ? Promise.resolve(undefined) : previewActionsRef.current.compactSession(...args), [preview.isActive]);
  const guardedundoTurn = useCallback((...args: Parameters<typeof undoTurn>) => preview.isActive() ? Promise.resolve(null) : previewActionsRef.current.undoTurn(...args), [preview.isActive]);
  const guardedrollbackToMessage = useCallback((...args: Parameters<typeof rollbackToMessage>) => preview.isActive() ? Promise.resolve(false) : previewActionsRef.current.rollbackToMessage(...args), [preview.isActive]);
  const guardededitAndResendMessage = useCallback((...args: Parameters<typeof editAndResendMessage>) => preview.isActive() ? Promise.resolve(false) : previewActionsRef.current.editAndResendMessage(...args), [preview.isActive]);
  const guardedretryTurn = useCallback((...args: Parameters<typeof retryTurn>) => preview.isActive() ? Promise.resolve(false) : previewActionsRef.current.retryTurn(...args), [preview.isActive]);
  const guardedLoadOlder = useCallback(() => preview.isActive() ? preview.loadOlder() : previewActionsRef.current.loadOlderMessages(), [preview.isActive, preview.loadOlder]);

  const updateReadinessRef = useRef({ isLoading, isSessionLoading, isReviewing, lastError, historyError });
  updateReadinessRef.current = { isLoading, isSessionLoading, isReviewing, lastError, historyError };
  const prepareForUpdate = useCallback(async (): Promise<void> => {
    const navigation = navigationRef.current;
    const assertIdle = () => {
      if (navigationRef.current !== navigation) throw new Error('Navigation changed while preparing the update. Try again.');
      const state = updateReadinessRef.current;
      if (state.isLoading || state.isSessionLoading || state.isReviewing || initializingRef.current
        || navigationPendingRef.current || sessionOperationRef.current || preview.isActive()
        || [...liveSessionsRef.current.values()].some((live) => live.runId || live.starting
          || live.loadingOlder || live.checkpointBusy || live.deltaBusy)) {
        throw new Error('Wait until all runs and session operations are idle before installing an update.');
      }
      if (state.lastError || state.historyError || staleSaveErrorsRef.current.size
        || [...liveSessionsRef.current.values()].some((live) => live.historyError)) {
        throw new Error('Resolve local state errors before installing an update.');
      }
    };
    assertIdle();
    const settingsSnapshot = settingsRef.current;
    await persistenceQueueRef.current;
    assertIdle();
    await settingsQueueRef.current;
    assertIdle();
    if (settingsRef.current !== settingsSnapshot || settingsRef.current !== confirmedSettingsRef.current) {
      throw new Error('Settings changed or could not be saved while preparing the update.');
    }
    const sessions = [...liveSessionsRef.current.values()];
    for (const live of sessions) {
      flushPending(live);
      if (live.dirty) await persistData(live.data);
      assertIdle();
    }
    if (settingsRef.current !== settingsSnapshot || settingsRef.current !== confirmedSettingsRef.current
      || [...liveSessionsRef.current.values()].some((live) => !sessions.includes(live) || live.dirty || live.persistenceBusy
        || live.frame !== null || live.checkpointBusy || live.deltaBusy)) {
      throw new Error('Local state changed while preparing the update. Save and try again.');
    }
  }, [preview.isActive, flushPending, persistData]);

  return {
    prepareForUpdate,
    messages: preview.page?.data.messages ?? data.messages,
    isHistoryPreview: preview.page !== null || preview.loading,
    historyView: preview.page ? 'around' as const : 'latest' as const,
    jumpToMessage: preview.jump,
    openMessage, openSearchResult, loadLatestMessages,
    artifacts: data.artifacts,
    activeArtifact,
    activeArtifactId,
    setActiveArtifactId: selectArtifact,
    subagents,
    backgroundTasks,
    refreshBackgroundTasks,
    killBackgroundTask,
    activeSkills,
    listSkills,
    listAllArtifacts,
    deepSearch,
    isStreaming,
    runStatus: preview.page ? null : runStatus,
    isReviewing,
    reviewSession: guardedreviewSession,
    stopRequested,
    isLoading,
    isSessionLoading,
    loadingOlderMessages: preview.page ? preview.loading : loadingOlderMessages,
    loadOlderMessages: guardedLoadOlder,
    historyError: preview.error ?? historyError,
    hasOlderMessages: (preview.page?.window.offset ?? data.historyWindow?.offset ?? 0) > 0,
    totalMessageCount: preview.page?.window.total ?? ((data.historyWindow?.offset ?? 0) + data.messages.length),
    sessions,
    sessionActivities,
    currentSessionId: data.session.id,
    sessionTitle: data.session.title,
    modelOverride: data.modelOverride,
    settings,
    workspaceCwd,
    effectiveConfig,
    lastError,
    clearError: () => setLastError(null),
    sendMessage: guardedsendMessage,
    selectCouncilWinner: guardedselectCouncilWinner,
    stopAgent,
    compactSession: guardedcompactSession,
    undoTurn: guardedundoTurn,
    rollbackToMessage: guardedrollbackToMessage,
    editAndResendMessage: guardededitAndResendMessage,
    retryTurn: guardedretryTurn,
    initAgentsMd,
    readAgentsMd,
    saveAgentsMd,
    newSession,
    switchSession,
    deleteSession,
    renameSession,
    generateSessionTitle,
    togglePinSession,
    exportSession,
    clearCurrentSession,
    updateSettings,
    selectModel,
    updateRunConfig,
    clearModelOverride,
    toggleFavorite,
    addCustomModel,
    refreshModels,
    selectWorkspace,
    clearWorkspace,
    listWorkspaceFiles,
    gitStatus,
    refreshGitStatus,
    commitWorkspaceChanges,
  };
}
