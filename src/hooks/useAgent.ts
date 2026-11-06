import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import { playTaskCompleteChime, showDesktopNotification } from '../utils/audioChime';
import type {
  AgentEvent,
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
  return {
    ...message, isStreaming: false, runStatus: 'cancelled',
    toolCalls: message.toolCalls?.map((tool) => tool.status === 'running' || tool.status === 'pending'
      ? { ...tool, status: 'cancelled' } : tool),
    councilTurn: message.councilTurn ? {
      ...message.councilTurn,
      candidateA: { ...message.councilTurn.candidateA, isStreaming: false },
      candidateB: { ...message.councilTurn.candidateB, isStreaming: false },
    } : undefined,
  };
}

function restoreSession(loaded: SessionData, reviews: Map<string, SelfImprovementEvent>): SessionData {
  return {
    ...loaded,
    messages: loaded.messages.map((message) => {
      const restored = message.role === 'assistant' && (message.isStreaming || (message.runId !== undefined && !message.runStatus))
        ? cancelledMessage(message) : { ...message, isStreaming: false };
      const review = reviews.get(JSON.stringify([loaded.session.id, message.runId ?? message.id]));
      return review ? { ...restored, selfImprovement: review } : restored;
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

  const dataRef = useRef(data);
  const settingsRef = useRef(settings);
  const workspaceRef = useRef(workspaceCwd);
  const sessionIdRef = useRef(data.session.id);
  const currentRunRef = useRef<string | null>(null);
  const streamingMessageIdRef = useRef<string | null>(null);
  const textBufferRef = useRef({ content: '', reasoning: '' });
  const councilBufferRef = useRef({ contentA: '', reasoningA: '', contentB: '', reasoningB: '' });
  const terminalBufferRef = useRef<Record<string, string>>({});
  const frameRef = useRef<number | null>(null);
  const runStartTimeRef = useRef<number | null>(null);
  const persistenceQueueRef = useRef<Promise<void>>(Promise.resolve());
  const navigationRef = useRef(0);
  const sessionOperationRef = useRef(false);
  const reviewResultsRef = useRef(new Map<string, SelfImprovementEvent>());

  const mutateData = useCallback((mutator: (current: SessionData) => SessionData) => {
    const next = mutator(dataRef.current);
    dataRef.current = next;
    sessionIdRef.current = next.session.id;
    setData(next);
  }, []);

  const refreshSessions = useCallback(async () => {
    try {
      setSessions(await invoke<Session[]>('list_sessions'));
    } catch (error) {
      setLastError(`Could not list sessions: ${String(error)}`);
    }
  }, []);

  const persistData = useCallback((value: SessionData): Promise<void> => {
    const queued = persistenceQueueRef.current.then(async () => {
      try {
        await invoke('save_session', { data: value });
        await refreshSessions();
      } catch (error) {
        setLastError(`Could not save session: ${String(error)}`);
        throw error;
      }
    });
    persistenceQueueRef.current = queued.catch(() => undefined);
    return queued;
  }, [refreshSessions]);

  const flushPending = useCallback(() => {
    if (frameRef.current !== null) {
      cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    }
    const text = textBufferRef.current;
    const terminal = terminalBufferRef.current;
    const council = councilBufferRef.current;
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
    textBufferRef.current = { content: '', reasoning: '' };
    terminalBufferRef.current = {};
    councilBufferRef.current = { contentA: '', reasoningA: '', contentB: '', reasoningB: '' };
    mutateData((current) => {
      let messages = current.messages;
      const messageId = streamingMessageIdRef.current;
      if (messageId && (text.content || text.reasoning)) {
        messages = messages.map((message) => message.id === messageId
          ? {
              ...message,
              content: message.content + text.content,
              reasoning: (message.reasoning || '') + text.reasoning,
            }
          : message);
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
  }, [mutateData]);

  const scheduleFlush = useCallback(() => {
    if (frameRef.current === null) {
      frameRef.current = requestAnimationFrame(flushPending);
    }
  }, [flushPending]);

  useEffect(() => {
    let active = true;
    const navigationAtStart = navigationRef.current;
    setIsLoading(true);
    void Promise.allSettled([
      invoke<AgentSettings>('get_settings'),
      invoke<Session[]>('list_sessions'),
    ]).then(([settingsResult, sessionsResult]) => {
      if (!active) return;
      if (settingsResult.status === 'fulfilled') {
        settingsRef.current = settingsResult.value;
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
          invoke<SessionData>('load_session', { sessionId: latest.id })
            .then((loaded) => {
              if (!active || navigationRef.current !== navigationAtStart) return;
              const normalized = restoreSession(loaded, reviewResultsRef.current);
              dataRef.current = normalized;
              sessionIdRef.current = normalized.session.id;
              setData(normalized);
              const sessCwd = normalized.session.workspaceCwd || '';
              setWorkspaceCwd(sessCwd);
              workspaceRef.current = sessCwd;
              invoke('set_workspace_cwd', { newCwd: sessCwd }).catch(() => {});
            })
            .catch(() => {});
        } else {
          // Fresh start without sessions: no workspace by default
          setWorkspaceCwd('');
          workspaceRef.current = '';
          invoke('set_workspace_cwd', { newCwd: '' }).catch(() => {});
        }
      }
      setIsLoading(false);
    });
    return () => { active = false; };
  }, [mutateData]);

  useEffect(() => {
    const unlistenPromise = listen<AgentEvent>('agent-event', ({ payload }) => {
      const scope = payload.data;
      if (scope.runId !== currentRunRef.current || scope.sessionId !== sessionIdRef.current) return;
      switch (payload.type) {
        case 'chunk':
          textBufferRef.current.content += payload.data.text;
          scheduleFlush();
          break;
        case 'reasoning':
          textBufferRef.current.reasoning += payload.data.text;
          scheduleFlush();
          break;
        case 'tool_update':
          mutateData((current) => ({
            ...current,
            messages: current.messages.map((message) => {
              if (message.id !== streamingMessageIdRef.current) return message;
              const tools = [...(message.toolCalls || [])];
              const index = tools.findIndex((tool) => tool.id === payload.data.tool.id);
              if (index >= 0) tools[index] = payload.data.tool;
              else tools.push(payload.data.tool);
              return { ...message, toolCalls: tools };
            }),
          }));
          break;
        case 'artifact': {
          const artifact = payload.data.artifact;
          if (artifact.type === 'terminal' && artifact.content.length > 0) {
            delete terminalBufferRef.current[artifact.id];
          }
          mutateData((current) => {
            const artifacts = [...current.artifacts];
            const index = artifacts.findIndex((item) => item.id === artifact.id);
            if (index >= 0) artifacts[index] = artifact;
            else artifacts.unshift(artifact);
            return { ...current, artifacts };
          });
          setActiveArtifactId(artifact.id);
          break;
        }
        case 'terminal_output':
          terminalBufferRef.current[payload.data.artifactId] = (terminalBufferRef.current[payload.data.artifactId] || '') + payload.data.delta;
          scheduleFlush();
          break;
        case 'subagent_update': {
          const sub = payload.data.subagent;
          setSubagents((prev) => {
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
          setActiveSkills((prev) => {
            if (prev.some((item) => item.name.toLowerCase() === skill.name.toLowerCase())) return prev;
            return [...prev, skill];
          });
          break;
        }
        case 'council_chunk': {
          const { candidate, kind, text: chunkText } = payload.data;
          if (candidate === 'A') {
            if (kind === 'reasoning') councilBufferRef.current.reasoningA += chunkText;
            else councilBufferRef.current.contentA += chunkText;
          } else {
            if (kind === 'reasoning') councilBufferRef.current.reasoningB += chunkText;
            else councilBufferRef.current.contentB += chunkText;
          }
          scheduleFlush();
          break;
        }
        case 'council_done': {
          const { candidate, durationMs } = payload.data;
          mutateData((current) => ({
            ...current,
            messages: current.messages.map((m) => {
              if (m.id !== streamingMessageIdRef.current || !m.councilTurn) return m;
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
          runFailedRef.current = true;
          setLastError(payload.data.message);
          break;
        case 'done': {
          flushPending();
          const elapsedMs = runStartTimeRef.current ? Date.now() - runStartTimeRef.current : 0;
          runStartTimeRef.current = null;
          const currentSettings = settingsRef.current;
          if (elapsedMs > 2500) {
            if (currentSettings?.notificationsEnabled !== false && typeof document !== 'undefined' && document.hidden) {
              // Preview the assistant reply only — never echo the user's own prompt.
              const previewText = [...dataRef.current.messages].reverse().find((m) => m.role === 'assistant' && m.content.trim())?.content.slice(0, 100);
              void showDesktopNotification('AetherAgent: Task Complete', previewText || 'Agent has completed execution.');
            }
            if (currentSettings?.soundChimeEnabled === true) {
              void playTaskCompleteChime();
            }
          }
          mutateData((current) => ({
            ...current,
            session: {
              ...current.session,
              updatedAt: Date.now(),
              messageCount: current.messages.length,
              preview: [...current.messages].reverse().find((message) => message.content.trim())?.content.slice(0, 240),
            },
            messages: current.messages.map((message) => message.id === streamingMessageIdRef.current ? {
              ...message, isStreaming: false,
              runStatus: payload.data.cancelled ? 'cancelled' : payload.data.successful === false || runFailedRef.current ? 'failed' : 'completed',
            } : message),
          }));
          setIsStreaming(false);
          setStopRequested(false);
          currentRunRef.current = null;
          streamingMessageIdRef.current = null;
          void persistData(dataRef.current).catch(() => undefined);
          void refreshGitStatus();
          break;
        }
      }
    });
    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
      if (frameRef.current !== null) cancelAnimationFrame(frameRef.current);
    };
  }, [flushPending, mutateData, persistData, scheduleFlush]);

  useEffect(() => {
    const unlisten = listen<SelfImprovementEvent>('agent:self-improvement', ({ payload }) => {
      const key = JSON.stringify([payload.session_id, payload.run_id]);
      reviewResultsRef.current.delete(key);
      reviewResultsRef.current.set(key, payload);
      if (reviewResultsRef.current.size > 256) {
        const oldest = reviewResultsRef.current.keys().next().value;
        if (oldest !== undefined) reviewResultsRef.current.delete(oldest);
      }
      if (payload.session_id !== sessionIdRef.current) return;
      if (!dataRef.current.messages.some((message) => message.role === 'assistant' && (message.runId === payload.run_id || message.id === payload.run_id))) return;
      mutateData((current) => ({
        ...current,
        messages: current.messages.map((message) => message.role === 'assistant' && (message.runId === payload.run_id || message.id === payload.run_id)
          ? { ...message, selfImprovement: payload }
          : message),
      }));
      void persistData(dataRef.current).catch(() => undefined);
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
    },
  ) => {
    const prompt = rawPrompt.trim();
    if (/^\/(review|reflect)$/i.test(prompt)) return reviewSession();
    const currentSettings = settingsRef.current;
    const effective = resolveEffective(currentSettings, dataRef.current.modelOverride);
    if ((!prompt && (!attachments || attachments.length === 0)) || currentRunRef.current || sessionOperationRef.current || !currentSettings || !effective) {
      if (sessionOperationRef.current) setLastError('Wait for the current session operation to finish.');
      else if (!effective) setLastError('Configure a provider and model before sending a message.');
      return false;
    }
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
    let formattedPrompt = prompt;
    if (attachments && attachments.length > 0) {
      const list = attachments
        .map((a) => `- ${a.isDir ? '[Directory]' : '[File]'} \`${a.path}\``)
        .join('\n');
      formattedPrompt = prompt
        ? `${prompt}\n\n## Attached context:\n${list}`
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
    currentRunRef.current = runId;
    runFailedRef.current = false;
    runStartTimeRef.current = Date.now();
    streamingMessageIdRef.current = assistantId;
    setIsStreaming(true);
    setStopRequested(false);

    if (options?.council && options.candidateA && options.candidateB) {
      const requestA: AgentRunRequest = {
        runId,
        sessionId: dataRef.current.session.id,
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
      };
      const requestB: AgentRunRequest = {
        runId,
        sessionId: dataRef.current.session.id,
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
      };
      try {
        await persistData(dataRef.current);
        await invoke('send_council_message', { requestA, requestB });
      } catch (error) {
        setLastError(`Failed to start council agent: ${String(error)}`);
        mutateData((current) => ({
          ...current,
          messages: current.messages.map((message) => message.id === assistantId
            ? { ...message, content: `Error: ${String(error)}`, isStreaming: false, runStatus: 'failed' }
            : message),
        }));
        currentRunRef.current = null;
        streamingMessageIdRef.current = null;
        setIsStreaming(false);
        void persistData(dataRef.current).catch(() => undefined);
      }
      return true;
    }

    const request: AgentRunRequest = {
      runId,
      sessionId: dataRef.current.session.id,
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
    };
    try {
      await persistData(dataRef.current);
      await invoke('send_message', { request });
    } catch (error) {
      setLastError(`Failed to start agent: ${String(error)}`);
      mutateData((current) => ({
        ...current,
        messages: current.messages.map((message) => message.id === assistantId
          ? { ...message, content: message.content || `Error: ${String(error)}`, isStreaming: false, runStatus: 'failed' }
          : message),
      }));
      currentRunRef.current = null;
      streamingMessageIdRef.current = null;
      setIsStreaming(false);
      void persistData(dataRef.current).catch(() => undefined);
    }
    return true;
  }, [mutateData, persistData, reviewSession]);

  const stopAgent = useCallback(async () => {
    const runId = currentRunRef.current;
    if (!runId || stopRequested) return;
    setStopRequested(true);
    try {
      await invoke('abort_agent', { runId });
    } catch (error) {
      setLastError(`Could not stop run: ${String(error)}`);
      setStopRequested(false);
    }
  }, [stopRequested]);

  const detachCurrentRun = useCallback(() => {
    flushPending();
    const runId = currentRunRef.current;
    if (!runId) return;
    mutateData((current) => ({
      ...current,
      messages: current.messages.map((message) => message.role === 'assistant' && message.runId === runId
        ? cancelledMessage(message) : message),
    }));
    currentRunRef.current = null;
    streamingMessageIdRef.current = null;
    void invoke('abort_agent', { runId }).catch(() => undefined);
  }, [flushPending, mutateData]);

  const resetSession = useCallback((persistPrevious: boolean): string => {
    navigationRef.current += 1;
    detachCurrentRun();
    if (persistPrevious && dataRef.current.messages.length > 0) void persistData(dataRef.current).catch(() => undefined);
    textBufferRef.current = { content: '', reasoning: '' };
    councilBufferRef.current = { contentA: '', reasoningA: '', contentB: '', reasoningB: '' };
    terminalBufferRef.current = {};
    if (frameRef.current !== null) {
      cancelAnimationFrame(frameRef.current);
      frameRef.current = null;
    }
    const next = createSession(); // New session has NO workspace by default!
    dataRef.current = next;
    sessionIdRef.current = next.session.id;
    setData(next);
    setWorkspaceCwd('');
    workspaceRef.current = '';
    invoke('set_workspace_cwd', { newCwd: '' }).catch(() => {});
    setIsStreaming(false);
    setStopRequested(false);
    setActiveArtifactId(null);
    setLastError(null);
    setSubagents([]);
    setActiveSkills([]);
    return next.session.id;
  }, [detachCurrentRun, persistData]);

  const newSession = useCallback(() => resetSession(true), [resetSession]);

  const switchSession = useCallback(async (sessionId: string) => {
    if (sessionId === sessionIdRef.current) return;
    const navigation = ++navigationRef.current;
    detachCurrentRun();
    const previous = dataRef.current;
    textBufferRef.current = { content: '', reasoning: '' };
    councilBufferRef.current = { contentA: '', reasoningA: '', contentB: '', reasoningB: '' };
    terminalBufferRef.current = {};
    setIsStreaming(false);
    setStopRequested(false);
    try {
      if (previous.messages.length > 0) await persistData(previous);
      const loaded = await invoke<SessionData>('load_session', { sessionId });
      if (navigationRef.current !== navigation) return;
      const normalized = restoreSession(loaded, reviewResultsRef.current);
      dataRef.current = normalized;
      sessionIdRef.current = normalized.session.id;
      setData(normalized);
      setActiveArtifactId(normalized.artifacts[0]?.id || null);
      setLastError(null);
      setSubagents([]);
      setActiveSkills([]);
      const sessCwd = normalized.session.workspaceCwd || '';
      setWorkspaceCwd(sessCwd);
      workspaceRef.current = sessCwd;
      await invoke('set_workspace_cwd', { newCwd: sessCwd });
    } catch (error) {
      if (navigationRef.current === navigation) {
        setLastError(`Could not load session: ${String(error)}`);
      }
    }
  }, [detachCurrentRun, persistData]);

  const clearCurrentSession = useCallback(() => {
    const runId = currentRunRef.current;
    if (runId) void invoke('abort_agent', { runId }).catch(() => undefined);
    currentRunRef.current = null;
    streamingMessageIdRef.current = null;
    setIsStreaming(false);
    setStopRequested(false);
    mutateData((current) => ({
      ...current,
      session: { ...current.session, title: 'New Chat', messageCount: 0, preview: undefined },
      messages: [],
      artifacts: [],
    }));
    setActiveArtifactId(null);
    setSubagents([]);
    setActiveSkills([]);
    void persistData(dataRef.current).catch(() => undefined);
  }, [mutateData, persistData]);

  const updateSettings = useCallback(async (next: AgentSettings) => {
    const previous = settingsRef.current;
    settingsRef.current = next;
    setSettings(next);
    try {
      await invoke('save_settings', { settings: next });
    } catch (error) {
      settingsRef.current = previous;
      setSettings(previous);
      setLastError(`Could not save settings: ${String(error)}`);
      throw error;
    }
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
    const current = settingsRef.current;
    const clean = model.trim();
    if (!current || !clean) return;
    const next: AgentSettings = {
      ...current,
      providers: current.providers.map((provider) => provider.id === providerId
        ? { ...provider, customModels: [...new Set([...(provider.customModels || []), clean])] }
        : provider),
    };
    await updateSettings(next);
    selectModel(providerId, clean);
  }, [selectModel, updateSettings]);

  const refreshModels = useCallback(async (providerId: string) => {
    const current = settingsRef.current;
    const provider = current?.providers.find((item) => item.id === providerId);
    if (!current || !provider) throw new Error('Provider not found');
    const models = await invoke<string[]>('fetch_provider_models', { provider });
    const next: AgentSettings = {
      ...current,
      providers: current.providers.map((item) => item.id === providerId ? { ...item, models } : item),
    };
    await updateSettings(next);
    return models;
  }, [updateSettings]);

  const renameSession = useCallback(async (sessionId: string, title: string) => {
    try {
      if (sessionId === sessionIdRef.current) {
        mutateData((current) => ({
          ...current,
          session: { ...current.session, title },
        }));
        await persistData(dataRef.current);
        return;
      }
      await persistenceQueueRef.current;
      const renamed = await invoke<SessionData>('rename_session', { sessionId, title });
      await refreshSessions();
      if (sessionId === sessionIdRef.current) {
        mutateData((current) => ({ ...current, session: { ...current.session, title: renamed.session.title } }));
      }
    } catch (error) {
      setLastError(`Could not rename session: ${String(error)}`);
    }
  }, [mutateData, persistData, refreshSessions]);

  const togglePinSession = useCallback(async (sessionId: string) => {
    try {
      await persistenceQueueRef.current;
      const updated = await invoke<Session>('toggle_pin_session', { sessionId });
      if (sessionId === sessionIdRef.current) {
        mutateData((current) => ({
          ...current,
          session: {
            ...current.session,
            pinned: updated.pinned,
            pinnedAt: updated.pinnedAt,
          },
        }));
      }
      await refreshSessions();
    } catch (error) {
      setLastError(`Could not toggle pin on session: ${String(error)}`);
    }
  }, [mutateData, refreshSessions]);

  const deleteSession = useCallback(async (sessionId: string) => {
    if (sessionId === sessionIdRef.current) resetSession(false);
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
      let destination = requestedDestination?.trim() || null;
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
    const selected = await open({ directory: true, multiple: false, defaultPath: workspaceRef.current || undefined });
    if (typeof selected !== 'string') return;
    try {
      const cwd = await invoke<string>('set_workspace_cwd', { newCwd: selected });
      workspaceRef.current = cwd;
      setWorkspaceCwd(cwd);
      mutateData((current) => ({ ...current, session: { ...current.session, workspaceCwd: cwd } }));
    } catch (error) {
      setLastError(`Could not switch workspace: ${String(error)}`);
    }
  }, [mutateData]);

  const clearWorkspace = useCallback(async () => {
    try {
      await invoke('set_workspace_cwd', { newCwd: '' });
      workspaceRef.current = '';
      setWorkspaceCwd('');
      mutateData((current) => ({
        ...current,
        session: { ...current.session, workspaceCwd: undefined },
      }));
    } catch (error) {
      setLastError(`Could not clear workspace: ${String(error)}`);
    }
  }, [mutateData]);

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
      return [];
    }
  }, []);

  const deepSearch = useCallback(async (query: string, limit = 20): Promise<import('../types/agent').DeepSearchResult[]> => {
    try {
      return await invoke<import('../types/agent').DeepSearchResult[]>('deep_search', { query, limit });
    } catch (error) {
      console.error('Deep search error:', error);
      return [];
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
      dataRef.current = normalized;
      sessionIdRef.current = normalized.session.id;
      setData(normalized);
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
        preview: [...messages].reverse().find((message) => message.content.trim())?.content.slice(0, 240),
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
        preview: [...messages].reverse().find((message) => message.content.trim())?.content.slice(0, 240),
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
    if (currentRunRef.current) {
      const activeRun = currentRunRef.current;
      currentRunRef.current = null;
      setIsStreaming(false);
      setStopRequested(false);
      void invoke('abort_agent', { runId: activeRun }).catch(() => {});
    }
    if (sessionOperationRef.current) {
      setLastError('Wait for session operations before editing.');
      return false;
    }
    const clean = newContent.trim();
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
        preview: [...messages].reverse().find((message) => message.content.trim())?.content.slice(0, 240),
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

    return sendMessage(clean, attachments);
  }, [mutateData, persistData, sendMessage]);

  const retryTurn = useCallback(async (messageId: string): Promise<boolean> => {
    if (currentRunRef.current) {
      const activeRun = currentRunRef.current;
      currentRunRef.current = null;
      setIsStreaming(false);
      setStopRequested(false);
      void invoke('abort_agent', { runId: activeRun }).catch(() => {});
    }
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

    return editAndResendMessage(userMsg.id, userMsg.content);
  }, [editAndResendMessage]);

  const initAgentsMd = useCallback(async () => {
    try {
      const result = await invoke<string>('init_workspace_agents_md');
      return result;
    } catch (error) {
      setLastError(`Failed to initialize AGENTS.md: ${String(error)}`);
      throw error;
    }
  }, []);

  const readAgentsMd = useCallback(async () => {
    return await invoke<string | null>('read_workspace_agents_md');
  }, []);

  const saveAgentsMd = useCallback(async (content: string) => {
    await invoke('save_workspace_agents_md', { content });
  }, []);

  return {
    messages: data.messages,
    artifacts: data.artifacts,
    activeArtifact,
    activeArtifactId,
    setActiveArtifactId,
    subagents,
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
