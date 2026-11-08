import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import { liveSession, type LiveSession } from './useAgentLiveSession';
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
  SessionData,
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

function restoreSession(loaded: SessionData, reviews: Map<string, SelfImprovementEvent>): SessionData {
  return {
    ...loaded,
    messages: loaded.messages.map((message) => {
      const restored = message.role === 'assistant' && (message.isStreaming || (message.runId !== undefined && !message.runStatus))
        ? cancelledMessage(message) : { ...message, isStreaming: false };
      const review = reviews.get(JSON.stringify([loaded.session.id, message.runId ?? message.id]));
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

export function useAgent() {
  const [data, setData] = useState<SessionData>(() => createSession());
  const [sessions, setSessions] = useState<Session[]>([]);
  const [settings, setSettings] = useState<AgentSettings | null>(null);
  const [workspaceCwd, setWorkspaceCwd] = useState('');
  const [isStreaming, setIsStreaming] = useState(false);
  const [isReviewing, setIsReviewing] = useState(false);
  const runFailedRef = useRef(false);
  const [stopRequested, setStopRequested] = useState(false);
  const [isLoading, setIsLoading] = useState(true);
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
  const navigationRef = useRef(0);
  const sessionOperationRef = useRef(false);
  const initializingRef = useRef(true);
  const navigationPendingRef = useRef(false);
  const reviewResultsRef = useRef(new Map<string, SelfImprovementEvent>());
  const liveSessionsRef = useRef(new Map<string, LiveSession>());
  const viewedLive = useCallback(() => {
    let live = liveSessionsRef.current.get(sessionIdRef.current);
    if (!live) {
      live = liveSession(dataRef.current);
      liveSessionsRef.current.set(sessionIdRef.current, live);
    }
    return live;
  }, []);

  const publishLive = useCallback((live: LiveSession) => {
    if (sessionIdRef.current !== live.data.session.id) return;
    dataRef.current = live.data;
    currentRunRef.current = live.runId;
    streamingMessageIdRef.current = live.messageId;
    runStartTimeRef.current = live.startedAt;
    runFailedRef.current = live.failed;
    setData(live.data);
    setIsStreaming(live.runId !== null);
    setStopRequested(live.stopRequested);
    setSubagents(live.subagents);
    setActiveSkills(live.skills);
    setActiveArtifactId(live.artifactId);
  }, []);

  const mutateData = useCallback((mutator: (current: SessionData) => SessionData, target?: LiveSession) => {
    const live = target ?? viewedLive();
    live.data = mutator(live.data);
    live.authoritative = true;
    publishLive(live);
  }, [publishLive, viewedLive]);

  const selectArtifact = useCallback((value: string | null | ((current: string | null) => string | null)) => {
    const live = viewedLive();
    live.artifactId = typeof value === 'function' ? value(live.artifactId) : value;
    publishLive(live);
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

  const persistData = useCallback((value: SessionData, checkpoint = false): Promise<void> => {
    const queued = persistenceQueueRef.current.then(async () => {
      try {
        await invoke(checkpoint ? 'checkpoint_session' : 'save_session', { data: value });
        if (!checkpoint) await refreshSessions();
      } catch (error) {
        setLastError(`Could not save session: ${String(error)}`);
        throw error;
      }
    });
    persistenceQueueRef.current = queued.catch(() => undefined);
    return queued;
  }, [refreshSessions]);

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

  // One delayed snapshot per live session. Flush independently of RAF so hidden
  // windows also reach disk. A slow disk never queues an unbounded snapshot list.
  const scheduleCheckpoint = useCallback((live: LiveSession) => {
    const generation = live.checkpointGeneration;
    const arm = () => {
      if (live.checkpointGeneration !== generation || !live.runId || live.checkpointTimer !== null) return;
      live.checkpointTimer = setTimeout(() => {
        live.checkpointTimer = null;
        if (live.checkpointGeneration !== generation || !live.runId) return;
        if (live.checkpointBusy) { arm(); return; }
        flushPending(live);
        const snapshot = live.data;
        live.checkpointBusy = true;
        void persistData(snapshot, true).catch(() => undefined).finally(() => {
          live.checkpointBusy = false;
          if (live.runId && live.data !== snapshot) arm();
        });
      }, 2000);
    };
    arm();
  }, [flushPending, persistData]);

  const scheduleFlush = useCallback((live: LiveSession) => {
    if (live.frame === null) live.frame = requestAnimationFrame(() => flushPending(live));
  }, [flushPending]);

  useEffect(() => {
    let active = true;
    const navigationAtStart = navigationRef.current;
    setIsLoading(true);
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
          await invoke<SessionData>('load_session', { sessionId: latest.id })
            .then(async (loaded) => {
              if (!active || navigationRef.current !== navigationAtStart) return;
              const normalized = restoreSession(loaded, reviewResultsRef.current);
              const live = liveSession(normalized);
              liveSessionsRef.current.set(normalized.session.id, live);
              sessionIdRef.current = normalized.session.id;
              publishLive(live);
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
      }
    });
    return () => { active = false; };
  }, [mutateData, publishLive]);

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
          const noticeData = payload.data;
          if (noticeData.resetPartial) {
            live.text.content = '';
            live.text.reasoning = '';
          }
          mutate((current) => ({
            ...current,
            messages: current.messages.map((message) => {
              if (message.id !== live.messageId) return message;
              return {
                ...message,
                content: noticeData.resetPartial ? '' : message.content,
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
          live.text.content += payload.data.text;
          scheduleFlush(live);
          mutate((current) => {
            const streamingMsg = current.messages.find((m) => m.id === live.messageId);
            if (!streamingMsg || !streamingMsg.notice) return current;
            return {
              ...current,
              messages: current.messages.map((m) => m.id === live.messageId ? { ...m, notice: null } : m),
            };
          });
          break;
        case 'reasoning':
          live.text.reasoning += payload.data.text;
          scheduleFlush(live);
          mutate((current) => {
            const streamingMsg = current.messages.find((m) => m.id === live.messageId);
            if (!streamingMsg || !streamingMsg.notice) return current;
            return {
              ...current,
              messages: current.messages.map((m) => m.id === live.messageId ? { ...m, notice: null } : m),
            };
          });
          break;
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
          if (payload.data.tool.name === 'run_command' || payload.data.tool.name === 'manage_task') {
            void refreshBackgroundTasks();
          }
          break;
        case 'artifact': {
          const artifact = payload.data.artifact;
          if (artifact.type === 'terminal' && artifact.content.length > 0) {
            delete live.terminal[artifact.id];
          }
          mutate((current) => {
            const artifacts = [...current.artifacts];
            const index = artifacts.findIndex((item) => item.id === artifact.id);
            if (index >= 0) artifacts[index] = artifact;
            else artifacts.unshift(artifact);
            return { ...current, artifacts };
          });
          live.artifactId = artifact.id;
          break;
        }
        case 'terminal_output':
          live.terminal[payload.data.artifactId] = (live.terminal[payload.data.artifactId] || '') + payload.data.delta;
          scheduleFlush(live);
          break;
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
          const { candidate, kind, text: chunkText } = payload.data;
          if (candidate === 'A') {
            if (kind === 'reasoning') live.council.reasoningA += chunkText;
            else live.council.contentA += chunkText;
          } else {
            if (kind === 'reasoning') live.council.reasoningB += chunkText;
            else live.council.contentB += chunkText;
          }
          scheduleFlush(live);
          break;
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
          setLastError(payload.data.message);
          mutate((current) => ({
            ...current,
            messages: current.messages.map((message) => {
              if (message.id !== live.messageId) return message;
              return { ...message, notice: null };
            }),
          }));
          break;
        case 'done': {
          if (live.checkpointTimer !== null) { clearTimeout(live.checkpointTimer); live.checkpointTimer = null; }
          flushPending(live);
          const elapsedMs = live.startedAt ? Date.now() - live.startedAt : 0;
          live.startedAt = null;
          const currentSettings = settingsRef.current;
          if (elapsedMs > 2500) {
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
              messageCount: current.messages.length,
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

          live.stopRequested = false;
          live.runId = null;
          live.messageId = null;
          void persistData(live.data).catch(() => undefined);
          void refreshGitStatus();
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
  }, [flushPending, mutateData, persistData, scheduleFlush, scheduleCheckpoint, publishLive]);

  useEffect(() => {
    const unlisten = listen<SelfImprovementEvent>('agent:self-improvement', ({ payload }) => {
      const key = JSON.stringify([payload.session_id, payload.run_id]);
      reviewResultsRef.current.delete(key);
      reviewResultsRef.current.set(key, payload);
      if (reviewResultsRef.current.size > 256) {
        const oldest = reviewResultsRef.current.keys().next().value;
        if (oldest !== undefined) reviewResultsRef.current.delete(oldest);
      }
      const live = liveSessionsRef.current.get(payload.session_id);
      if (!live || !live.data.messages.some((message) => message.role === 'assistant' && (message.runId === payload.run_id || message.id === payload.run_id))) return;
      mutateData((current) => ({
        ...current,
        messages: current.messages.map((message) => message.role === 'assistant' && (message.runId === payload.run_id || message.id === payload.run_id)
          ? { ...message, selfImprovement: payload }
          : message),
      }), live);
      void persistData(live.data).catch(() => undefined);
    });
    const unlistenErrors = listen<{ run_id: string; session_id: string; message: string }>('agent:self-improvement-error', ({ payload }) => {
      if (payload.session_id === sessionIdRef.current) setLastError(`Self-improvement review failed: ${payload.message}`);
    });
    return () => {
      void unlisten.then((dispose) => dispose());
      void unlistenErrors.then((dispose) => dispose());
    };
  }, [mutateData, persistData]);

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
    },
  ) => {
    const prompt = (rawPrompt || '').trim();
    if (initializingRef.current || navigationPendingRef.current) {
      setLastError('Wait for local state to finish loading.');
      return false;
    }
    if (/^\/(review|reflect)$/i.test(prompt)) return reviewSession();
    const currentSettings = settingsRef.current;
    const effective = resolveEffective(currentSettings, dataRef.current.modelOverride);
    if ((!prompt && (!attachments || attachments.length === 0)) || currentRunRef.current || sessionOperationRef.current || !currentSettings || !effective) {
      if (sessionOperationRef.current) setLastError('Wait for the current session operation to finish.');
      else if (!effective) setLastError('Configure a provider and model before sending a message.');
      return false;
    }
    const live = viewedLive();
    const originWorkspace = live.data.session.workspaceCwd ?? workspaceRef.current;
    setLastError(null);
    const runId = makeId('run');
    const userMessage: ChatMessage = {
      id: makeId('msg'),
      role: 'user',
      content: prompt,
      attachments,
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
    let activePrompt = prompt;
    if (/^\/plan(\s+.*)?$/i.test(prompt)) {
      const task = prompt.replace(/^\/plan\s*/i, '').trim();
      activePrompt = `Please formulate a structured step-by-step execution plan for: ${task || 'the current task'}.\n\nFormat your plan inside a \`\`\`plan code block with checkable steps:\n\`\`\`plan\n- [ ] Step 1: <Description>\n- [ ] Step 2: <Description>\n- [ ] Step 3: <Description>\n\`\`\`\n\nThen proceed to carry out the steps sequentially, updating and reporting progress as each step completes.`;
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
    const requestMessages = [...dataRef.current.messages, apiUserMessage];
    const firstMessage = !dataRef.current.messages.some((message) => message.role === 'user');
    mutateData((current) => ({
      ...current,
      session: {
        ...current.session,
        title: firstMessage
          ? (prompt.split(/\s+/).join(' ').slice(0, 70) || (attachments?.[0]?.name ? `Context: ${attachments[0].name}` : 'New Chat'))
          : current.session.title,
        updatedAt: Date.now(),
      },
      messages: [...dataRef.current.messages, userMessage, assistantMessage],
    }));
    live.runId = runId;
    live.stopRequested = false;
    live.failed = false;
    live.startedAt = Date.now();
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
        autoCompactionEnabled: currentSettings.autoCompactionEnabled ?? true,
        autoCompactionThresholdTokens: currentSettings.autoCompactionThresholdTokens ?? 65536,
        executionMode: selectedExecutionMode,
      };
      try {
        await persistData(live.data);
        if (live.cancelledBeforeStart || live.runId !== runId) return true;
        live.starting = false;
        await invoke('send_council_message', { requestA, requestB });
      } catch (error) {
        if (live.runId !== runId) return true;
        setLastError(`Failed to start council agent: ${String(error)}`);
        mutateData((current) => ({
          ...current,
          messages: current.messages.map((message) => message.id === assistantId
            ? { ...message, content: `Error: ${String(error)}`, isStreaming: false, runStatus: 'failed' }
            : message),
        }), live);
        live.runId = null;
        live.messageId = null;
        publishLive(live);
        void persistData(live.data).catch(() => undefined);
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
      autoCompactionEnabled: currentSettings.autoCompactionEnabled ?? true,
      autoCompactionThresholdTokens: currentSettings.autoCompactionThresholdTokens ?? 65536,
      executionMode: selectedExecutionMode,
    };
    try {
      await persistData(live.data);
      if (live.cancelledBeforeStart || live.runId !== runId) return true;
      live.starting = false;
      await invoke('send_message', { request });
    } catch (error) {
      if (live.runId !== runId) return true;
      setLastError(`Failed to start agent: ${String(error)}`);
      mutateData((current) => ({
        ...current,
        messages: current.messages.map((message) => message.id === assistantId
          ? { ...message, content: message.content || `Error: ${String(error)}`, isStreaming: false, runStatus: 'failed' }
          : message),
      }), live);
      live.runId = null;
      live.messageId = null;
      publishLive(live);
      void persistData(live.data).catch(() => undefined);
    }
    return true;
  }, [mutateData, persistData, reviewSession, publishLive, viewedLive]);

  const stopAgent = useCallback(async () => {
    const live = viewedLive();
    const runId = live.runId;
    if (!runId || live.stopRequested) return;
    live.stopRequested = true;
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
      void persistData(live.data).catch(() => undefined);
      return;
    }
    try {
      await invoke('abort_agent', { runId });
    } catch (error) {
      setLastError(`Could not stop run: ${String(error)}`);
      live.stopRequested = false;
      publishLive(live);
    }

  }, [mutateData, persistData, publishLive, viewedLive]);

  const detachCurrentRun = useCallback(() => {
    const live = viewedLive();
    flushPending(live);
    const runId = live.runId;
    live.cancelledBeforeStart = true;
    live.subagents = live.subagents.map((sub) => sub.status === 'running' ? { ...sub, status: 'cancelled', completedAt: Date.now() } : sub);
    live.skills = [];
    live.data = { ...live.data, messages: live.data.messages.map((message) => message.runId === runId ? cancelledMessage(message) : message) };
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
    if (persistPrevious && previous.data.messages.length > 0) void persistData(previous.data).catch(() => undefined);
    const next = liveSession(createSession());
    liveSessionsRef.current.set(next.data.session.id, next);
    sessionIdRef.current = next.data.session.id;
    publishLive(next);
    setWorkspaceCwd('');
    workspaceRef.current = '';
    void invoke('set_workspace_cwd', { newCwd: '' }).catch((error) => setLastError(`Could not clear workspace: ${String(error)}`));
    setLastError(null);
    return next.data.session.id;
  }, [flushPending, persistData, publishLive, viewedLive]);

  const newSession = useCallback(() => resetSession(true), [resetSession]);

  const switchSession = useCallback(async (sessionId: string) => {
    const navigation = ++navigationRef.current;
    if (sessionId === sessionIdRef.current) { navigationPendingRef.current = false; return; }
    navigationPendingRef.current = true;
    const previous = viewedLive();
    flushPending(previous);
    try {
      if (previous.data.messages.length > 0) await persistData(previous.data);
      let next = liveSessionsRef.current.get(sessionId);
      if (!next?.authoritative && !next?.runId) {
        const loaded = await invoke<SessionData>('load_session', { sessionId });
        const current = liveSessionsRef.current.get(sessionId);
        next = current?.authoritative || current?.runId ? current : liveSession(restoreSession(loaded, reviewResultsRef.current));
      }
      if (!next) return;
      if (navigationRef.current !== navigation) return;
      liveSessionsRef.current.set(sessionId, next);
      sessionIdRef.current = sessionId;
      flushPending(next);
      publishLive(next);
      setLastError(null);
      const sessCwd = next.data.session.workspaceCwd || '';
      workspaceRef.current = sessCwd;
      setWorkspaceCwd(sessCwd);
      await invoke('set_workspace_cwd', { newCwd: sessCwd });
    } catch (error) {
      if (navigationRef.current === navigation) setLastError(`Could not load session: ${String(error)}`);
    } finally {
      if (navigationRef.current === navigation) navigationPendingRef.current = false;
    }
  }, [flushPending, persistData, publishLive, viewedLive]);

  const clearCurrentSession = useCallback(() => {
    navigationRef.current += 1;
    detachCurrentRun();
    mutateData((current) => ({
      ...current,
      session: { ...current.session, title: 'New Chat', messageCount: 0, preview: undefined },
      messages: [],
      artifacts: [],
    }));
    selectArtifact(null);
    setSubagents([]);
    setActiveSkills([]);
    void persistData(dataRef.current).catch(() => undefined);
  }, [mutateData, persistData, selectArtifact, detachCurrentRun]);

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
    try {
      const prepared = sessionId === sessionIdRef.current ? persistData(viewedLive().data) : persistenceQueueRef.current;
      const queued = prepared.then(async () => {
        const renamed = await invoke<SessionData>('rename_session', { sessionId, title });
        const origin = liveSessionsRef.current.get(sessionId);
        if (origin) mutateData((current) => ({ ...current, session: { ...current.session, title: renamed.session.title } }), origin);
        await refreshSessions();
      });
      persistenceQueueRef.current = queued.catch(() => undefined);
      await queued;
    } catch (error) {
      setLastError(`Could not rename session: ${String(error)}`);
    }
  }, [mutateData, persistData, refreshSessions, viewedLive]);

  const togglePinSession = useCallback(async (sessionId: string) => {
    try {
      const queued = persistenceQueueRef.current.then(async () => {
        const updated = await invoke<Session>('toggle_pin_session', { sessionId });
        const origin = liveSessionsRef.current.get(sessionId);
        if (origin) mutateData((current) => ({
          ...current,
          session: { ...current.session, pinned: updated.pinned, pinnedAt: updated.pinnedAt },
        }), origin);
        await refreshSessions();
      });
      persistenceQueueRef.current = queued.catch(() => undefined);
      await queued;
    } catch (error) {
      setLastError(`Could not toggle pin on session: ${String(error)}`);
    }
  }, [mutateData, refreshSessions]);

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
    return invoke<import('../types/agent').SkillInfo[]>('list_skills', { paths });
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
    }));
    void persistData(dataRef.current).catch(() => undefined);
  }, [mutateData, persistData]);

  const compactSession = useCallback(async () => {
    if (currentRunRef.current || sessionOperationRef.current) {
      setLastError('Stop the active run and wait for session operations before compacting.');
      return;
    }
    if (!dataRef.current.session.id || dataRef.current.messages.length < 3) {
      setLastError('Conversation requires at least 3 messages to compact.');
      return;
    }
    const sessionId = dataRef.current.session.id;
    const navigation = navigationRef.current;
    sessionOperationRef.current = true;
    setIsLoading(true);
    try {
      await persistData(dataRef.current);
      const updated = await invoke<SessionData>('compact_session', { sessionId });
      if (navigationRef.current !== navigation || sessionIdRef.current !== sessionId) return;
      const normalized: SessionData = {
        ...updated,
        messages: updated.messages.map((message) => ({ ...message, isStreaming: false })),
      };
      mutateData(() => normalized);
      setSessions((current) =>
        current.map((session) => (session.id === normalized.session.id ? normalized.session : session))
      );
    } catch (error) {
      setLastError(`Context compaction failed: ${String(error)}`);
    } finally {
      sessionOperationRef.current = false;
      setIsLoading(false);
    }
  }, [persistData]);

  const undoTurn = useCallback(async (): Promise<import('../types/agent').UndoTurnResult | null> => {
    if (currentRunRef.current || sessionOperationRef.current) {
      setLastError('Stop the active run and wait for session operations before undoing a turn.');
      return null;
    }
    const previous = dataRef.current;
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

    sessionOperationRef.current = true;
    setIsLoading(true);
    mutateData(() => next);
    try {
      await persistData(next);
      return { content: undone.content, attachments: undone.attachments };
    } catch {
      if (!currentRunRef.current && sessionIdRef.current === previous.session.id) {
        mutateData(() => previous);
      }
      return null;
    } finally {
      sessionOperationRef.current = false;
      setIsLoading(false);
    }
  }, [mutateData, persistData]);


  const rollbackToMessage = useCallback(async (targetMessageId: string): Promise<boolean> => {
    if (currentRunRef.current || sessionOperationRef.current) {
      setLastError('Stop the active run and wait for session operations before rolling back.');
      return false;
    }
    const previous = dataRef.current;
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
      await persistData(next);
      return true;
    } catch (error) {
      setLastError(`Could not rollback session: ${String(error)}`);
      return false;
    } finally {
      sessionOperationRef.current = false;
      setIsLoading(false);
    }
  }, [mutateData, persistData]);

  const editAndResendMessage = useCallback(async (messageId: string, newContent: string): Promise<boolean> => {
    const navigation = navigationRef.current;
    if (currentRunRef.current) detachCurrentRun();
    if (sessionOperationRef.current) {
      setLastError('Wait for session operations before editing.');
      return false;
    }
    const clean = (newContent || '').trim();
    if (!clean) return false;

    const previous = dataRef.current;
    const targetIdx = previous.messages.findIndex((m) => m.id === messageId);
    if (targetIdx < 0) return false;

    const targetMsg = previous.messages[targetIdx];
    const attachments = targetMsg.attachments;

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

    return sendMessage(clean, attachments);
  }, [mutateData, persistData, sendMessage, detachCurrentRun]);

  const retryTurn = useCallback(async (messageId: string): Promise<boolean> => {
    if (currentRunRef.current) detachCurrentRun();
    if (sessionOperationRef.current) {
      setLastError('Wait for session operations before retrying.');
      return false;
    }
    const previous = dataRef.current;
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
  }, [editAndResendMessage, detachCurrentRun]);

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

  return {
    messages: data.messages,
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
    isReviewing,
    reviewSession,
    stopRequested,
    isLoading,
    sessions,
    currentSessionId: data.session.id,
    sessionTitle: data.session.title,
    modelOverride: data.modelOverride,
    settings,
    workspaceCwd,
    effectiveConfig,
    lastError,
    clearError: () => setLastError(null),
    sendMessage,
    selectCouncilWinner,
    stopAgent,
    compactSession,
    undoTurn,
    rollbackToMessage,
    editAndResendMessage,
    retryTurn,
    initAgentsMd,
    readAgentsMd,
    saveAgentsMd,
    newSession,
    switchSession,
    deleteSession,
    renameSession,
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
