import { useState, useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ChatMessage, Artifact, AgentSettings, Session, AgentEvent, ToolCallItem } from '../types/agent';

export function useAgent() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [activeArtifactId, setActiveArtifactId] = useState<string | null>(null);
  const [isStreaming, setIsStreaming] = useState(false);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string>(() => `sess_${Date.now()}`);
  const [sessionTitle, setSessionTitle] = useState<string>('New Chat');
  const [settings, setSettings] = useState<AgentSettings | null>(null);
  const [workspaceCwd, setWorkspaceCwd] = useState<string>('');

  const messagesRef = useRef<ChatMessage[]>(messages);
  messagesRef.current = messages;

  const artifactsRef = useRef<Artifact[]>(artifacts);
  artifactsRef.current = artifacts;

  // 1. Initial Load
  useEffect(() => {
    // Load Settings
    invoke<AgentSettings>('get_settings').then(setSettings).catch(console.error);

    // Load Workspace CWD
    invoke<string>('get_workspace_cwd').then(setWorkspaceCwd).catch(console.error);

    // Load Sessions
    invoke<Session[]>('list_sessions').then(setSessions).catch(console.error);
  }, []);

  // 2. Listen to Backend Agent Events from Rust
  useEffect(() => {
    const unlistenPromise = listen<AgentEvent>('agent-event', (event) => {
      const ev = event.payload;

      if (ev.type === 'chunk') {
        const chunk = ev.data as string;
        setMessages((prev) => {
          const last = prev[prev.length - 1];
          if (last && last.role === 'assistant') {
            return [
              ...prev.slice(0, -1),
              { ...last, content: last.content + chunk }
            ];
          } else {
            return [
              ...prev,
              {
                id: `msg_${Date.now()}`,
                role: 'assistant',
                content: chunk,
                timestamp: Date.now()
              }
            ];
          }
        });
      } else if (ev.type === 'reasoning') {
        const reasoningChunk = ev.data as string;
        setMessages((prev) => {
          const last = prev[prev.length - 1];
          if (last && last.role === 'assistant') {
            return [
              ...prev.slice(0, -1),
              { ...last, reasoning: (last.reasoning || '') + reasoningChunk }
            ];
          } else {
            return [
              ...prev,
              {
                id: `msg_${Date.now()}`,
                role: 'assistant',
                content: '',
                reasoning: reasoningChunk,
                timestamp: Date.now()
              }
            ];
          }
        });
      } else if (ev.type === 'tool_update') {
        const item = ev.data as ToolCallItem;
        setMessages((prev) => {
          const last = prev[prev.length - 1];
          if (last && last.role === 'assistant') {
            const currentTools = last.toolCalls ? [...last.toolCalls] : [];
            const existingIdx = currentTools.findIndex((t) => t.id === item.id);
            if (existingIdx >= 0) {
              currentTools[existingIdx] = item;
            } else {
              currentTools.push(item);
            }
            return [
              ...prev.slice(0, -1),
              { ...last, toolCalls: currentTools }
            ];
          }
          return prev;
        });
      } else if (ev.type === 'artifact') {
        const artifact = ev.data as Artifact;
        setArtifacts((prev) => {
          const exists = prev.some((a) => a.id === artifact.id);
          if (exists) return prev;
          return [artifact, ...prev];
        });
        setActiveArtifactId(artifact.id);
      } else if (ev.type === 'done') {
        setIsStreaming(false);
        // Save session on finish
        invoke('save_session', {
          sessionId: currentSessionId,
          title: sessionTitle,
          messages: messagesRef.current,
          artifacts: artifactsRef.current
        }).then(() => {
          invoke<Session[]>('list_sessions').then(setSessions);
        });
      } else if (ev.type === 'error') {
        setIsStreaming(false);
        const errMsg = ev.data as string;
        setMessages((prev) => [
          ...prev,
          {
            id: `err_${Date.now()}`,
            role: 'assistant',
            content: `⚠️ Error: ${errMsg}`,
            timestamp: Date.now()
          }
        ]);
      }
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [currentSessionId, sessionTitle]);

  // 3. User Actions
  const sendMessage = async (prompt: string) => {
    if (!prompt.trim() || isStreaming || !settings) return;

    const activeProvider = settings.providers.find((p) => p.id === settings.activeProviderId);
    if (!activeProvider) {
      alert('No active LLM provider configured. Please check Settings.');
      return;
    }

    const userMsg: ChatMessage = {
      id: `msg_${Date.now()}`,
      role: 'user',
      content: prompt,
      timestamp: Date.now()
    };

    const newMessages = [...messages, userMsg];
    setMessages(newMessages);
    setIsStreaming(true);

    // Auto title for the first message
    if (messages.length === 0) {
      const generatedTitle = prompt.slice(0, 30).trim() + (prompt.length > 30 ? '...' : '');
      setSessionTitle(generatedTitle);
    }

    try {
      await invoke('send_message', {
        provider: activeProvider,
        systemPrompt: settings.systemPrompt || null,
        messages: newMessages
      });
    } catch (err: any) {
      setIsStreaming(false);
      setMessages((prev) => [
        ...prev,
        {
          id: `err_${Date.now()}`,
          role: 'assistant',
          content: `⚠️ Failed to send message: ${err}`,
          timestamp: Date.now()
        }
      ]);
    }
  };

  const stopAgent = async () => {
    try {
      await invoke('abort_agent');
      setIsStreaming(false);
    } catch (err) {
      console.error(err);
    }
  };

  const newSession = () => {
    if (isStreaming) stopAgent();
    const newId = `sess_${Date.now()}`;
    setCurrentSessionId(newId);
    setSessionTitle('New Chat');
    setMessages([]);
    setArtifacts([]);
    setActiveArtifactId(null);
  };

  const switchSession = async (sessionId: string) => {
    if (isStreaming) stopAgent();
    try {
      const [loadedMsgs, loadedArts] = await invoke<[ChatMessage[], Artifact[]]>('load_session', { sessionId });
      setCurrentSessionId(sessionId);
      const found = sessions.find((s) => s.id === sessionId);
      if (found) setSessionTitle(found.title);
      setMessages(loadedMsgs);
      setArtifacts(loadedArts);
      setActiveArtifactId(loadedArts.length > 0 ? loadedArts[0].id : null);
    } catch (err) {
      console.error('Failed to load session:', err);
    }
  };

  const updateSettings = async (newSettings: AgentSettings) => {
    setSettings(newSettings);
    await invoke('save_settings', { settings: newSettings });
  };

  const selectWorkspace = async () => {
    try {
      // In Tauri, we can use dialog plugin or invoke
      const selected = await invoke<string>('get_workspace_cwd');
      setWorkspaceCwd(selected);
    } catch (err) {
      console.error(err);
    }
  };

  const deleteSession = async (sessionId: string) => {
    try {
      await invoke('delete_session', { sessionId });
      setSessions((prev) => prev.filter((s) => s.id !== sessionId));
      if (currentSessionId === sessionId) {
        newSession();
      }
    } catch (err) {
      console.error('Failed to delete session:', err);
    }
  };

  const clearCurrentSession = () => {
    setMessages([]);
    setArtifacts([]);
    setActiveArtifactId(null);
  };

  const fetchProviderModels = async (baseUrl: string, apiKey: string): Promise<string[]> => {
    try {
      return await invoke<string[]>('fetch_provider_models', { baseUrl, apiKey });
    } catch (err: any) {
      throw new Error(err.message || String(err));
    }
  };

  const switchActiveProvider = async (providerId: string) => {
    if (!settings) return;
    const updated: AgentSettings = {
      ...settings,
      activeProviderId: providerId,
    };
    await updateSettings(updated);
  };

  const activeArtifact = artifacts.find((a) => a.id === activeArtifactId) || null;

  return {
    messages,
    artifacts,
    activeArtifact,
    activeArtifactId,
    setActiveArtifactId,
    isStreaming,
    sessions,
    currentSessionId,
    sessionTitle,
    settings,
    workspaceCwd,
    sendMessage,
    stopAgent,
    newSession,
    switchSession,
    deleteSession,
    clearCurrentSession,
    fetchProviderModels,
    switchActiveProvider,
    updateSettings,
    selectWorkspace
  };
}
