import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  AtSign,
  ChevronDown,
  BookOpen,
  Brain,
  ChevronUp,
  Clock,
  Download,
  File,
  FileCode2,
  Folder,
  Globe,
  Image as ImageIcon,
  Mic,
  MicOff,
  MoreHorizontal,
  PanelLeft,
  PanelRight,
  Paperclip,
  Play,
  Plus,
  Search,
  Send,
  Settings,
  SlidersHorizontal,
  Square,
  Terminal,
  Trash2,
  Users,
  X,
} from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import type { AttachedContext, BackgroundTask, EffectiveRunConfig, SkillInfo, SubagentProgress, UndoTurnResult } from '../types/agent';
import { MessageRow } from './MessageRow';
import { AetherLogo } from './AetherLogo';
import { SubagentTreeCard } from './SubagentTreeCard';
import { BackgroundTasksCard } from './BackgroundTasksCard';
import { TimelineScrubber } from './TimelineScrubber';
import { useSpeechRecognition } from '../hooks/useSpeechRecognition';

interface ChatCanvasProps {
  messages: import('../types/agent').ChatMessage[];
  isStreaming: boolean;
  stopRequested: boolean;
  onSendMessage: (
    text: string,
    attachments?: AttachedContext[],
    options?: {
      council?: boolean;
      candidateA?: { provider: import('../types/agent').ProviderConfig; thinking?: boolean; reasoningEffort?: import('../types/agent').ReasoningEffort };
      candidateB?: { provider: import('../types/agent').ProviderConfig; thinking?: boolean; reasoningEffort?: import('../types/agent').ReasoningEffort };
    },
  ) => Promise<boolean>;
  onStop: () => void;
  onSelectArtifact: (id: string) => void;
  onClearSession: () => void;
  onNewSession: () => string;
  onOpenSettings: () => void;
  onOpenModelPicker: () => void;
  onOpenSpotlight?: () => void;
  onOpenLiveShell?: () => void;
  onOpenHistory?: () => void;
  onOpenTools?: () => void;
  onOpenMemory?: () => void;
  onCompactSession?: () => Promise<void>;
  onReviewSession?: () => Promise<boolean>;
  isReviewing?: boolean;
  onUndoTurn?: () => Promise<UndoTurnResult | null>;
  onRetryTurn?: (messageId: string) => Promise<boolean>;
  onEditAndResendMessage?: (messageId: string, newContent: string) => Promise<boolean>;
  onRollbackToMessage?: (messageId: string) => Promise<boolean>;
  onInitAgentsMd?: () => Promise<string>;
  onExportSession?: (format: 'html' | 'markdown' | 'json', destination?: string) => void;
  onSelectCouncilWinner?: (messageId: string, winner: 'A' | 'B') => void;
  sidebarOpen?: boolean;
  onToggleSidebar?: () => void;
  effectiveConfig: EffectiveRunConfig | null;
  sessionTitle: string;
  artifactCount: number;
  isArtifactOpen: boolean;
  onToggleArtifacts: () => void;
  onListFiles: (query: string, limit?: number) => Promise<string[]>;
  subagents?: SubagentProgress[];
  backgroundTasks?: BackgroundTask[];
  onKillBackgroundTask?: (taskId: string) => void;
  onRefreshBackgroundTasks?: () => void;
  activeSkills?: SkillInfo[];
  onListSkills?: (paths?: string[]) => Promise<SkillInfo[]>;
  settings?: import('../types/agent').AgentSettings | null;
  onUpdateOverride?: (override: Partial<import('../types/agent').SessionModelOverride>) => void;
  onSelectModel?: (providerId: string, model: string) => void;
  onRenameSession?: (sessionId: string, title: string) => Promise<void>;
  currentSessionId?: string | null;
}

export interface SlashItem {
  id: string;
  category: 'COMMANDS' | 'TOOLS & AGENTS' | 'SKILLS';
  command: string;
  description: string;
  usage?: string;
  iconType: 'terminal' | 'skill';
  needsPrompt?: boolean;
}

const COMMAND_ITEMS: SlashItem[] = [
  { id: 'cmd_plan', category: 'COMMANDS', command: '/plan', description: 'Formulate an interactive step-by-step execution plan', usage: 'Formulate execution plan\n/plan <task goal>', iconType: 'terminal', needsPrompt: true },
  { id: 'cmd_tasks', category: 'COMMANDS', command: '/tasks', description: 'List and inspect active background tasks and processes', usage: 'Manage background tasks\n/tasks', iconType: 'terminal' },
  { id: 'cmd_export', category: 'COMMANDS', command: '/export', description: 'Export this conversation transcript to Markdown (.md)', usage: 'Export conversation\n/export', iconType: 'terminal' },
  { id: 'cmd_review', category: 'COMMANDS', command: '/review', description: 'Review the latest completed turn for reusable lessons', iconType: 'terminal' },
  { id: 'cmd_reflect', category: 'COMMANDS', command: '/reflect', description: 'Review the latest completed turn (alias of /review)', iconType: 'terminal' },
  {
    id: 'cmd_compact',
    category: 'COMMANDS',
    command: '/compact',
    description: 'Compress this conversation context',
    usage: 'Compress conversation context\n/compact',
    iconType: 'terminal',
  },
  {
    id: 'cmd_compress',
    category: 'COMMANDS',
    command: '/compress',
    description: 'Compress this conversation context (usage: /compress)',
    usage: 'Compress conversation context\n/compress',
    iconType: 'terminal',
  },
  {
    id: 'cmd_new',
    category: 'COMMANDS',
    command: '/new',
    description: 'Start a new desktop chat (usage: /new [name])',
    usage: 'Start a new chat\n/new [session_name]',
    iconType: 'terminal',
    needsPrompt: true,
  },
  {
    id: 'cmd_history',
    category: 'COMMANDS',
    command: '/history',
    description: 'Show conversation history',
    usage: 'Display history summary\n/history',
    iconType: 'terminal',
  },
  {
    id: 'cmd_save',
    category: 'COMMANDS',
    command: '/save',
    description: 'Save the current transcript to JSON (usage: /save [file])',
    usage: 'Save the current transcript to JSON\n/save [filename]',
    iconType: 'terminal',
    needsPrompt: true,
  },
  {
    id: 'cmd_retry',
    category: 'COMMANDS',
    command: '/retry',
    description: 'Retry the last message (resend to agent)',
    usage: 'Resend last prompt\n/retry',
    iconType: 'terminal',
  },
  {
    id: 'cmd_undo',
    category: 'COMMANDS',
    command: '/undo',
    description: 'Undo the last user turn and restore its prompt',
    usage: 'Undo last turn\n/undo',
    iconType: 'terminal',
  },
  {
    id: 'cmd_title',
    category: 'COMMANDS',
    command: '/title',
    description: 'Rename the current session (usage: /title [name])',
    usage: 'Rename current session\n/title [name]',
    iconType: 'terminal',
    needsPrompt: true,
  },
  {
    id: 'cmd_clear',
    category: 'COMMANDS',
    command: '/clear',
    description: 'Clear the current conversation history',
    usage: 'Clear messages\n/clear',
    iconType: 'terminal',
  },
  {
    id: 'cmd_model',
    category: 'COMMANDS',
    command: '/model',
    description: 'Select model and reasoning/thinking settings',
    usage: 'Open model picker\n/model',
    iconType: 'terminal',
  },
  {
    id: 'cmd_settings',
    category: 'COMMANDS',
    command: '/settings',
    description: 'Open provider and agent configuration',
    usage: 'Open settings dialog\n/settings',
    iconType: 'terminal',
  },
  {
    id: 'cmd_stop',
    category: 'COMMANDS',
    command: '/stop',
    description: 'Stop the active turn and background processes',
    usage: 'Stop generation\n/stop',
    iconType: 'terminal',
  },
];

const TOOL_ITEMS: SlashItem[] = [
  {
    id: 'tool_tools',
    category: 'TOOLS & AGENTS',
    command: '/tools',
    description: 'Inspect native tools and configured MCP servers',
    usage: 'Inspect active tools\n/tools',
    iconType: 'terminal',
  },
  {
    id: 'tool_skills',
    category: 'TOOLS & AGENTS',
    command: '/skills',
    description: 'Inspect discovered local skills',
    usage: 'Inspect skills list\n/skills',
    iconType: 'terminal',
  },
  {
    id: 'tool_memory',
    category: 'TOOLS & AGENTS',
    command: '/memory',
    description: 'Read and edit workspace AGENTS.md memory',
    usage: 'Review workspace memory\n/memory',
    iconType: 'terminal',
  },
  {
    id: 'tool_init',
    category: 'TOOLS & AGENTS',
    command: '/init',
    description: 'Generate AGENTS.md when no workspace instructions exist',
    usage: 'Generate AGENTS.md if missing\n/init',
    iconType: 'terminal',
  },
];

interface QueuedPrompt {
  id: string;
  prompt: string;
  attachments?: AttachedContext[];
}

export function ChatCanvas({
  messages,
  isStreaming,
  stopRequested,
  onSendMessage,
  onStop,
  onSelectArtifact,
  onClearSession,
  onNewSession,
  onOpenSettings,
  onOpenModelPicker,
  onOpenSpotlight,
  onOpenLiveShell,
  onOpenHistory,
  onOpenTools,
  onOpenMemory,
  onCompactSession,
  onReviewSession,
  isReviewing = false,
  onUndoTurn,
  onRetryTurn,
  onEditAndResendMessage,
  onRollbackToMessage,
  onInitAgentsMd,
  onExportSession,
  onSelectCouncilWinner,
  sidebarOpen = true,
  onToggleSidebar,
  effectiveConfig,
  sessionTitle,
  artifactCount,
  isArtifactOpen,
  onToggleArtifacts,
  onListFiles,
  subagents = [],
  backgroundTasks = [],
  onKillBackgroundTask,
  onRefreshBackgroundTasks,
  activeSkills = [],
  onListSkills,
  settings,
  onUpdateOverride,
  onSelectModel,
  onRenameSession,
  currentSessionId,
}: ChatCanvasProps) {
  const [input, setInput] = useState('');
  const [cursor, setCursor] = useState(0);
  const [activeIndex, setActiveIndex] = useState(0);
  const [menuDismissed, setMenuDismissed] = useState(false);
  const [councilMode, setCouncilMode] = useState(false);
  const [councilModelA, setCouncilModelA] = useState<{ providerId: string; model: string } | null>(null);
  const [councilModelB, setCouncilModelB] = useState<{ providerId: string; model: string } | null>(null);
  const [moreMenuOpen, setMoreMenuOpen] = useState(false);
  const [queuedPrompts, setQueuedPrompts] = useState<QueuedPrompt[]>([]);
  const [stoppedWithQueue, setStoppedWithQueue] = useState(false);
  const prevStreamingRef = useRef(isStreaming);
  const wasStopRequestedRef = useRef(false);

  useEffect(() => {
    if (stopRequested) {
      wasStopRequestedRef.current = true;
    }
  }, [stopRequested]);

  // Reset queue when active session changes
  useEffect(() => {
    setQueuedPrompts([]);
    setStoppedWithQueue(false);
  }, [currentSessionId]);

  // Auto-dispatch next queued prompt when streaming ends (unless user explicitly stopped)
  useEffect(() => {
    if (prevStreamingRef.current && !isStreaming) {
      if (wasStopRequestedRef.current) {
        wasStopRequestedRef.current = false;
        if (queuedPrompts.length > 0) {
          setStoppedWithQueue(true);
        }
      } else if (queuedPrompts.length > 0 && !isReviewing) {
        setStoppedWithQueue(false);
        const nextItem = queuedPrompts[0];
        setQueuedPrompts((prev) => prev.slice(1));
        onSendMessage(nextItem.prompt, nextItem.attachments);
      }
    }
    prevStreamingRef.current = isStreaming;
  }, [isStreaming, queuedPrompts, isReviewing, onSendMessage]);

  const handleResumeQueue = useCallback(() => {
    if (queuedPrompts.length === 0 || isStreaming || isReviewing) return;
    setStoppedWithQueue(false);
    const nextItem = queuedPrompts[0];
    setQueuedPrompts((prev) => prev.slice(1));
    onSendMessage(nextItem.prompt, nextItem.attachments);
  }, [queuedPrompts, isStreaming, isReviewing, onSendMessage]);

  const handleEditQueued = useCallback((id: string) => {
    const item = queuedPrompts.find((q) => q.id === id);
    if (!item) return;
    setQueuedPrompts((prev) => prev.filter((q) => q.id !== id));
    setInput(item.prompt);
    if (item.attachments) {
      setAttachedContexts(item.attachments);
    }
    requestAnimationFrame(() => {
      textareaRef.current?.focus();
    });
  }, [queuedPrompts]);

  useEffect(() => {
    const handleInsertPrompt = (e: Event) => {
      const customEvent = e as CustomEvent<{ text: string; autoSend?: boolean }>;
      if (!customEvent.detail?.text) return;
      const { text, autoSend } = customEvent.detail;
      if (autoSend && !isStreaming && !isReviewing) {
        void onSendMessage(text);
      } else {
        setInput((prev) => (prev ? `${prev}\n\n${text}` : text));
        requestAnimationFrame(() => {
          if (textareaRef.current) {
            textareaRef.current.focus();
            const len = textareaRef.current.value.length;
            textareaRef.current.setSelectionRange(len, len);
          }
        });
      }
    };

    const handleClearComposer = () => {
      setInput('');
      setAttachedContexts([]);
    };

    const handleFocusComposer = () => {
      textareaRef.current?.focus();
    };

    window.addEventListener('aether:insert-prompt', handleInsertPrompt);
    window.addEventListener('aether:clear-composer', handleClearComposer);
    window.addEventListener('aether:focus-composer', handleFocusComposer);
    return () => {
      window.removeEventListener('aether:insert-prompt', handleInsertPrompt);
      window.removeEventListener('aether:clear-composer', handleClearComposer);
      window.removeEventListener('aether:focus-composer', handleFocusComposer);
    };
  }, [isStreaming, isReviewing, onSendMessage]);

  const handleRemoveQueued = useCallback((id: string) => {
    setQueuedPrompts((prev) => prev.filter((q) => q.id !== id));
  }, []);

  const availableModelsList = useMemo(() => {
    if (!settings) return [];
    const list: { providerId: string; providerName: string; model: string }[] = [];
    for (const p of settings.providers) {
      const models = [...new Set([p.model, ...(p.models || []), ...(p.customModels || [])])].filter(Boolean);
      for (const m of models) {
        list.push({ providerId: p.id, providerName: p.name, model: m });
      }
    }
    return list;
  }, [settings]);

  useEffect(() => {
    if (!councilModelA && effectiveConfig?.provider) {
      setCouncilModelA({
        providerId: effectiveConfig.provider.id,
        model: effectiveConfig.provider.model,
      });
    }
  }, [effectiveConfig, councilModelA]);

  useEffect(() => {
    if (councilModelB) return;
    const activeModel = effectiveConfig?.provider.model;
    // Never silently compare the active model against itself.
    const other = availableModelsList.find(
      (item) => item.model !== activeModel && item.model !== councilModelA?.model,
    );
    if (other) {
      setCouncilModelB({
        providerId: other.providerId,
        model: other.model,
      });
    }
  }, [availableModelsList, effectiveConfig, councilModelA, councilModelB]);

  const councilActiveModel = councilModelA?.model ?? effectiveConfig?.provider.model;
  const councilSecondCandidate = councilActiveModel
    ? availableModelsList.find((item) => item.model !== councilActiveModel)
    : undefined;
  const [fileOptions, setFileOptions] = useState<string[]>([]);
  const [loadingFiles, setLoadingFiles] = useState(false);
  const [attachedContexts, setAttachedContexts] = useState<AttachedContext[]>([]);
  const [addContextOpen, setAddContextOpen] = useState(false);
  const [skillsMenuOpen, setSkillsMenuOpen] = useState(false);
  const [availableSkills, setAvailableSkills] = useState<SkillInfo[]>([]);
  const [thinkingMenuOpen, setThinkingMenuOpen] = useState(false);
  const parentRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const stickToBottomRef = useRef(true);
  const [showJumpBottom, setShowJumpBottom] = useState(false);
  const [isDraggingOver, setIsDraggingOver] = useState(false);

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    if (!isDraggingOver) setIsDraggingOver(true);
  };

  const handleDragLeave = (e: React.DragEvent) => {
    e.preventDefault();
    if (e.currentTarget.contains(e.relatedTarget as Node)) return;
    setIsDraggingOver(false);
  };

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    setIsDraggingOver(false);
    const droppedFiles = Array.from(e.dataTransfer?.files || []);
    if (droppedFiles.length === 0) return;

    const realPaths = droppedFiles
      .map((f) => (f as any).path)
      .filter((p): p is string => typeof p === 'string' && p.trim().length > 0);

    if (realPaths.length > 0 && typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
      try {
        const contexts = await invoke<AttachedContext[]>('inspect_paths', { paths: realPaths });
        setAttachedContexts((prev) => {
          const existing = new Set(prev.map((c) => c.path));
          const unique = contexts.filter((c) => !existing.has(c.path));
          return [...prev, ...unique];
        });
        return;
      } catch (err) {
        console.warn('Failed to inspect dropped paths from HTML5 drop', err);
      }
    }

    const newContexts: AttachedContext[] = [];
    for (const f of droppedFiles) {
      if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
        try {
          const buf = await f.arrayBuffer();
          const att = await invoke<AttachedContext>('save_pasted_attachment', {
            name: f.name,
            bytes: Array.from(new Uint8Array(buf)),
          });
          newContexts.push(att);
          continue;
        } catch {
          // fallback
        }
      }
      const fullPath = (f as any).path || f.name;
      const isDir = !f.name.includes('.') && f.size === 0;
      newContexts.push({
        id: `ctx-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        name: f.name,
        path: fullPath,
        isDir,
      });
    }

    setAttachedContexts((prev) => {
      const existing = new Set(prev.map((c) => c.path));
      const unique = newContexts.filter((c) => !existing.has(c.path));
      return [...prev, ...unique];
    });
  };

  const handlePaste = useCallback(async (e: React.ClipboardEvent<HTMLTextAreaElement> | ClipboardEvent) => {
    const clipboardData = e.clipboardData;
    if (!clipboardData) return;

    const items = clipboardData.items;
    const files = clipboardData.files;

    const fileList: File[] = [];
    if (files && files.length > 0) {
      for (let i = 0; i < files.length; i++) {
        const f = files[i];
        if (f) fileList.push(f);
      }
    } else if (items && items.length > 0) {
      for (let i = 0; i < items.length; i++) {
        const item = items[i];
        if (item.kind === 'file') {
          const f = item.getAsFile();
          if (f) fileList.push(f);
        }
      }
    }

    // Normal text paste, allow native textarea paste
    if (fileList.length === 0) return;

    // Prevent pasting raw binary file content into textarea
    e.preventDefault();

    const newContexts: AttachedContext[] = [];

    for (const file of fileList) {
      const osPath = (file as any).path;
      if (typeof osPath === 'string' && osPath.trim().length > 0) {
        try {
          if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
            const inspected = await invoke<AttachedContext[]>('inspect_paths', { paths: [osPath] });
            if (inspected && inspected.length > 0) {
              newContexts.push(inspected[0]);
              continue;
            }
          }
        } catch (err) {
          console.warn('inspect_paths failed for pasted file path', err);
        }
      }

      try {
        const buf = await file.arrayBuffer();
        const bytes = Array.from(new Uint8Array(buf));
        let filename = file.name;
        if (!filename || filename === 'image.png' || filename === 'blob') {
          const ext = file.type.includes('jpeg') ? 'jpg' : file.type.includes('webp') ? 'webp' : 'png';
          const now = new Date();
          const dateStr = now.toISOString().replace(/[-:T.]/g, '').slice(0, 14);
          filename = `pasted_image_${dateStr}.${ext}`;
        }

        if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
          const att = await invoke<AttachedContext>('save_pasted_attachment', {
            name: filename,
            bytes,
          });
          newContexts.push(att);
        } else {
          newContexts.push({
            id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
            name: filename,
            path: filename,
            isDir: false,
          });
        }
      } catch (err) {
        console.error('Failed to process pasted attachment', err);
      }
    }

    if (newContexts.length > 0) {
      setAttachedContexts((prev) => {
        const existing = new Set(prev.map((c) => c.path));
        const unique = newContexts.filter((c) => !existing.has(c.path));
        return [...prev, ...unique];
      });
    }
  }, []);

  // Native Tauri drag & drop event listener
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const setupTauriDrop = async () => {
      try {
        if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
          const { getCurrentWebview } = await import('@tauri-apps/api/webview');
          unlisten = await getCurrentWebview().onDragDropEvent(async (event) => {
            if (event.payload.type === 'enter' || event.payload.type === 'over') {
              setIsDraggingOver(true);
            } else if (event.payload.type === 'leave') {
              setIsDraggingOver(false);
            } else if (event.payload.type === 'drop') {
              setIsDraggingOver(false);
              const paths = event.payload.paths;
              if (paths && paths.length > 0) {
                try {
                  const contexts = await invoke<AttachedContext[]>('inspect_paths', { paths });
                  setAttachedContexts((prev) => {
                    const existing = new Set(prev.map((c) => c.path));
                    const unique = contexts.filter((c) => !existing.has(c.path));
                    return [...prev, ...unique];
                  });
                } catch (err) {
                  console.error('Failed to inspect dropped paths', err);
                }
              }
            }
          });
        }
      } catch (err) {
        // Outside Tauri or test env
      }
    };

    setupTauriDrop();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Global paste listener when user presses Ctrl+V anywhere in ChatCanvas
  useEffect(() => {
    const onGlobalPaste = (e: ClipboardEvent) => {
      if (e.target === textareaRef.current) return;
      const targetTag = (e.target as HTMLElement)?.tagName;
      if (targetTag === 'INPUT' || targetTag === 'TEXTAREA') return;
      handlePaste(e);
    };

    window.addEventListener('paste', onGlobalPaste);
    return () => window.removeEventListener('paste', onGlobalPaste);
  }, [handlePaste]);

  // Preload skills automatically on mount and whenever streaming finishes
  useEffect(() => {
    if (onListSkills && !isStreaming) {
      onListSkills()
        .then((skills) => setAvailableSkills(skills))
        .catch((err) => console.error('Failed to preload skills', err));
    }
  }, [onListSkills, isStreaming]);

  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 140,
    overscan: 1,
    getItemKey: (index) => messages[index]?.id || index,
  });

  const handleJumpToBottom = useCallback(() => {
    stickToBottomRef.current = true;
    setShowJumpBottom(false);
    if (parentRef.current) {
      parentRef.current.scrollTop = parentRef.current.scrollHeight;
    }
    if (messages.length > 0) {
      virtualizer.scrollToIndex(messages.length - 1, { align: 'end' });
    }
  }, [messages.length, virtualizer]);

  const handleJumpToMessage = useCallback((index: number) => {
    stickToBottomRef.current = false;
    virtualizer.scrollToIndex(index, { align: 'start' });
  }, [virtualizer]);

  useEffect(() => {
    if (messages.length === 0) {
      setShowJumpBottom(false);
      return;
    }
    // Only auto-scroll on message changes if user is actively stuck to bottom
    if (stickToBottomRef.current) {
      requestAnimationFrame(() => virtualizer.scrollToIndex(messages.length - 1, { align: 'end' }));
    }
  }, [messages.length]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    setShowJumpBottom(false);
    stickToBottomRef.current = true;
  }, [sessionTitle]);

  useEffect(() => {
    if (!stickToBottomRef.current || !parentRef.current || messages.length === 0) return;
    const frame = requestAnimationFrame(() => {
      const element = parentRef.current;
      if (element && stickToBottomRef.current) element.scrollTop = element.scrollHeight;
    });
    return () => cancelAnimationFrame(frame);
  }, [messages]);

  // Slash entries are native operations; skills remain available through Add Context.
  const allSlashItems: SlashItem[] = useMemo(
    () => [...COMMAND_ITEMS, ...TOOL_ITEMS],
    [],
  );

  const slashQuery = useMemo(() => {
    if (!input.startsWith('/') || input.includes('\n')) return null;
    const spaceIdx = input.indexOf(' ');
    if (spaceIdx !== -1) return null;
    return input.slice(1).toLowerCase();
  }, [input]);

  const slashMatches = useMemo(() => {
    if (slashQuery === null) return [];
    if (!slashQuery) return allSlashItems;
    return allSlashItems.filter(
      (item) =>
        item.command.toLowerCase().includes(slashQuery) ||
        item.description.toLowerCase().includes(slashQuery),
    );
  }, [slashQuery, allSlashItems]);

  const groupedMatches = useMemo(() => {
    const groups: { category: 'COMMANDS' | 'TOOLS & AGENTS' | 'SKILLS'; items: SlashItem[] }[] = [];
    const catNames: Array<'COMMANDS' | 'TOOLS & AGENTS' | 'SKILLS'> = ['COMMANDS', 'TOOLS & AGENTS', 'SKILLS'];
    for (const cat of catNames) {
      const items = slashMatches.filter((item) => item.category === cat);
      if (items.length > 0) {
        groups.push({ category: cat, items });
      }
    }
    return groups;
  }, [slashMatches]);

  const slashOpen = !menuDismissed && slashMatches.length > 0;

  const { isListening, toggleListening } = useSpeechRecognition({
    onTranscript: (spoken) => {
      setInput((prev) => (prev ? `${prev} ${spoken}` : spoken));
    },
  });

  // Auto-scroll active item into view during keyboard navigation
  useEffect(() => {
    if (slashOpen) {
      const el = document.querySelector(`[data-slash-item="${activeIndex}"]`);
      el?.scrollIntoView({ block: 'nearest' });
    }
  }, [activeIndex, slashOpen]);

  const mentionMatch = useMemo(() => /(?:^|\s)@([^\s@]*)$/.exec(input.slice(0, cursor)), [input, cursor]);
  const mentionQuery = mentionMatch?.[1] || '';
  const mentionOpen = !menuDismissed && Boolean(mentionMatch);

  useEffect(() => {
    setActiveIndex(0);
  }, [input, cursor]);

  useEffect(() => {
    if (!mentionMatch) {
      setFileOptions([]);
      setLoadingFiles(false);
      return;
    }
    let active = true;
    setLoadingFiles(true);
    const timer = window.setTimeout(() => {
      onListFiles(mentionQuery, 30).then((files) => {
        if (active) setFileOptions(files);
      }).catch(() => {
        if (active) setFileOptions([]);
      }).finally(() => {
        if (active) setLoadingFiles(false);
      });
    }, 120);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [mentionMatch, mentionQuery, onListFiles]);

  useEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    textarea.style.height = 'auto';
    textarea.style.height = `${Math.min(textarea.scrollHeight, 180)}px`;
  }, [input]);

  useEffect(() => {
    const handleGlobalClick = (event: MouseEvent) => {
      const target = event.target as HTMLElement;
      if (!target.closest('[data-context-menu]')) {
        setAddContextOpen(false);
        setSkillsMenuOpen(false);
      }
      if (!target.closest('[data-thinking-menu]')) {
        setThinkingMenuOpen(false);
      }
      if (!target.closest('[data-more-menu]')) {
        setMoreMenuOpen(false);
      }
    };
    window.addEventListener('mousedown', handleGlobalClick);
    return () => window.removeEventListener('mousedown', handleGlobalClick);
  }, []);

  const recentModels = useMemo(() => {
    if (!settings) return [];
    const list: { providerId: string; providerName: string; model: string; key: string }[] = [];
    for (const provider of settings.providers) {
      const models = [...new Set([provider.model, ...(provider.models || []), ...(provider.customModels || [])].filter(Boolean))];
      for (const m of models) {
        list.push({ providerId: provider.id, providerName: provider.name, model: m, key: `${provider.id}:${m}` });
      }
    }
    const favSet = new Set(settings.favoriteModels);
    return list
      .sort((a, b) => Number(favSet.has(b.key)) - Number(favSet.has(a.key)))
      .slice(0, 12);
  }, [settings]);

  const handleAddFiles = async () => {
    setAddContextOpen(false);
    try {
      const selected = await open({ multiple: true });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      const newAtts: AttachedContext[] = paths.map((path) => {
        const normalized = path.replace(/\\/g, '/');
        const name = normalized.split('/').pop() || normalized;
        return {
          id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
          path,
          name,
          isDir: false,
        };
      });
      setAttachedContexts((prev) => {
        const existing = new Set(prev.map((c) => c.path));
        const unique = newAtts.filter((c) => !existing.has(c.path));
        return [...prev, ...unique];
      });
    } catch (error) {
      console.error('Failed to select files', error);
    }
  };

  const handleAddFolder = async () => {
    setAddContextOpen(false);
    try {
      const selected = await open({ directory: true, multiple: false });
      if (!selected || typeof selected !== 'string') return;
      const normalized = selected.replace(/\\/g, '/');
      const name = normalized.split('/').pop() || normalized;
      const newAtt: AttachedContext = {
        id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        path: selected,
        name,
        isDir: true,
      };
      setAttachedContexts((prev) => {
        if (prev.some((c) => c.path === selected)) return prev;
        return [...prev, newAtt];
      });
    } catch (error) {
      console.error('Failed to select folder', error);
    }
  };

  const handleAddMention = () => {
    setAddContextOpen(false);
    const needsSpace = input !== '' && !input.endsWith(' ');
    const next = needsSpace ? `${input} @` : `${input}@`;
    setInput(next);
    setCursor(next.length);
    textareaRef.current?.focus();
  };

  const handleOpenSkillsList = async () => {
    setAddContextOpen(false);
    if (onListSkills) {
      try {
        const skills = await onListSkills();
        setAvailableSkills(skills);
        setSkillsMenuOpen(true);
      } catch (error) {
        console.error('Failed to list skills', error);
      }
    }
  };

  const handleSelectSkillAction = (skill: SkillInfo) => {
    setSkillsMenuOpen(false);
    setInput((prev) => {
      const prefix = prev ? `${prev}\n` : '';
      return `${prefix}Please follow the '${skill.name}' skill to execute this task: `;
    });
    textareaRef.current?.focus();
  };

  const handleAddBrowser = () => {
    setAddContextOpen(false);
    const url = window.prompt('Enter public URL to inspect / fetch:');
    if (url && (url || '').trim()) {
      setInput((prev) => `${prev ? `${prev}\n` : ''}Fetch and analyze web page: ${(url || '').trim()}`);
      textareaRef.current?.focus();
    }
  };

  const handleSelectSlashItem = (item: SlashItem) => {
    if (item.needsPrompt) {
      const nextVal = `${item.command} `;
      setInput(nextVal);
      setCursor(nextVal.length);
      setMenuDismissed(true);
      requestAnimationFrame(() => {
        textareaRef.current?.focus();
        textareaRef.current?.setSelectionRange(nextVal.length, nextVal.length);
      });
    } else {
      executeSlashCommand(item);
    }
  };

  const executeSlashCommand = (item: SlashItem, argText?: string) => {
    setInput('');
    setMenuDismissed(false);
    setCursor(0);

    const arg = argText?.trim() || '';

    switch (item.command) {
      case '/clear':
        onClearSession();
        break;
      case '/new': {
        const newSessionId = onNewSession();
        if (arg && onRenameSession) {
          void onRenameSession(newSessionId, arg);
        }
        break;
      }
      case '/model':
        onOpenModelPicker();
        break;
      case '/settings':
        onOpenSettings();
        break;
      case '/stop':
        onStop();
        break;
      case '/review':
      case '/reflect':
        if (onReviewSession) void onReviewSession();
        else void onSendMessage(item.command);
        break;
      case '/compact':
      case '/compress':
        if (onCompactSession) {
          void onCompactSession();
        }
        break;
      case '/history':
        if (onOpenHistory) {
          onOpenHistory();
        }
        break;
      case '/tools':
        if (onOpenTools) {
          onOpenTools();
        }
        break;
      case '/tasks':
        if (onRefreshBackgroundTasks) {
          onRefreshBackgroundTasks();
        }
        void onSendMessage('List and report all background tasks using manage_task with action list.');
        break;
      case '/skills':
        handleOpenSkillsList();
        break;
      case '/init':
        if (onInitAgentsMd) {
          void onInitAgentsMd().then((msg) => {
            alert(msg);
          }).catch((err) => {
            alert(String(err));
          });
        }
        break;
      case '/memory':
        if (onOpenMemory) {
          onOpenMemory();
        }
        break;
      case '/retry':
        if (onUndoTurn) {
          void onUndoTurn().then(async (undone) => {
            if (undone) {
              await onSendMessage(undone.content || '', undone.attachments);
            }
          });
        }
        break;
      case '/undo':
        if (onUndoTurn) {
          void onUndoTurn().then((undone) => {
            if (undone) {
              setInput(undone.content || '');
              setAttachedContexts(undone.attachments || []);
              setCursor(undone.content.length);
              requestAnimationFrame(() => {
                textareaRef.current?.focus();
                textareaRef.current?.setSelectionRange(undone.content.length, undone.content.length);
              });
            }
          });
        }
        break;
      case '/title':
        if (arg && onRenameSession && currentSessionId) {
          onRenameSession(currentSessionId, arg);
        }
        break;
      case '/save':
        if (onExportSession) {
          onExportSession('json', arg || undefined);
        }
        break;
      case '/export':
        if (onExportSession) {
          onExportSession('markdown', arg || undefined);
        }
        break;
      default:
        break;
    }
  };

  const send = async () => {
    const draft = input;
    const draftAttachments = attachedContexts;
    const clean = (draft || '').trim();
    const slashItem = clean.startsWith('/')
      ? allSlashItems.find((item) => item.command.toLowerCase() === clean.split(/\s+/)[0].toLowerCase())
      : undefined;
    // /stop must stay reachable while a turn streams so the run can always be interrupted.
    const interruptsRun = slashItem?.command === '/stop';
    if ((!clean && draftAttachments.length === 0) || isReviewing) return;

    if (!isStreaming) {
      stickToBottomRef.current = true;
      setShowJumpBottom(false);
    }

    if (isStreaming && !interruptsRun) {
      setQueuedPrompts((prev) => [
        ...prev,
        {
          id: `queue-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
          prompt: draft,
          attachments: draftAttachments.length > 0 ? draftAttachments : undefined,
        },
      ]);
      setStoppedWithQueue(false);
      setInput('');
      setAttachedContexts([]);
      setCursor(0);
      return;
    }

    const clearIfUnchanged = (accepted: boolean) => {
      if (!accepted) return;
      setInput((current) => (current === draft ? '' : current));
      setAttachedContexts((current) => (current === draftAttachments ? [] : current));
      setCursor(0);
    };

    if (slashItem) {
      const args = clean.split(/\s+/).slice(1).join(' ').trim();
      executeSlashCommand(slashItem, args);
      setAttachedContexts([]);
      return;
    }

    if (councilMode && councilModelA && councilModelB && settings) {
      const provA = settings.providers.find((p) => p.id === councilModelA.providerId) || effectiveConfig?.provider;
      const provB = settings.providers.find((p) => p.id === councilModelB.providerId) || effectiveConfig?.provider;
      if (provA && provB) {
        const accepted = await onSendMessage(draft, draftAttachments.length > 0 ? draftAttachments : undefined, {
          council: true,
          candidateA: {
            provider: { ...provA, model: councilModelA.model },
            thinking: effectiveConfig?.thinking ?? false,
            reasoningEffort: effectiveConfig?.reasoningEffort ?? 'medium',
          },
          candidateB: {
            provider: { ...provB, model: councilModelB.model },
            thinking: effectiveConfig?.thinking ?? false,
            reasoningEffort: effectiveConfig?.reasoningEffort ?? 'medium',
          },
        });
        clearIfUnchanged(accepted);
        return;
      }
    }

    const accepted = await onSendMessage(
      draft,
      draftAttachments.length > 0 ? draftAttachments : undefined,
    );
    clearIfUnchanged(accepted);
  };

  const selectMention = (path: string) => {
    if (!mentionMatch) return;
    const tokenStart = input.slice(0, cursor).lastIndexOf('@');
    if (tokenStart < 0) return;
    const next = `${input.slice(0, tokenStart)}@${path} ${input.slice(cursor)}`;
    const nextCursor = tokenStart + path.length + 2;
    setInput(next);
    setCursor(nextCursor);
    setMenuDismissed(true);
    requestAnimationFrame(() => {
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(nextCursor, nextCursor);
    });
  };

  const handleSelectCouncilWinner = useCallback((messageId: string, winner: 'A' | 'B') => {
    onSelectCouncilWinner?.(messageId, winner);
  }, [onSelectCouncilWinner]);

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const itemCount = slashOpen ? slashMatches.length : mentionOpen ? fileOptions.length : 0;
    if (itemCount > 0 && ['ArrowDown', 'ArrowUp'].includes(event.key)) {
      event.preventDefault();
      const direction = event.key === 'ArrowUp' ? -1 : 1;
      setActiveIndex((index) => (index + direction + itemCount) % itemCount);
      return;
    }
    if (event.key === 'Tab' && slashOpen && slashMatches[activeIndex]) {
      event.preventDefault();
      handleSelectSlashItem(slashMatches[activeIndex]);
      return;
    }
    if (event.key === 'Escape' && (slashOpen || mentionOpen)) {
      event.preventDefault();
      setMenuDismissed(true);
      return;
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      if (slashOpen && slashMatches[activeIndex]) {
        handleSelectSlashItem(slashMatches[activeIndex]);
      } else if (mentionOpen && fileOptions[activeIndex]) {
        selectMention(fileOptions[activeIndex]);
      } else {
        send();
      }
    }
  };

  return (
    <div
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      className="relative flex h-full min-w-0 flex-1 flex-col bg-[#0d1117]"
    >
      {/* Drag & Drop Visual Overlay */}
      {isDraggingOver && (
        <div className="absolute inset-0 z-50 flex flex-col items-center justify-center bg-[#0d1117]/90 backdrop-blur-sm border-2 border-dashed border-[#58a6ff] m-2 rounded-2xl pointer-events-none animate-fadeIn select-none">
          <div className="flex h-14 w-14 items-center justify-center rounded-2xl bg-[#161b22] border border-[#30363d] shadow-2xl mb-3">
            <FileCode2 size={28} className="text-[#58a6ff] animate-bounce" />
          </div>
          <p className="text-sm font-semibold text-[#f0f6fc]">Drop files or folders here</p>
          <p className="text-xs text-[#8b949e] mt-1">Automatically attach as context to your prompt</p>
        </div>
      )}
      <header className="z-20 flex h-11 shrink-0 items-center justify-between border-b border-[#30363d] bg-[#0d1117] px-2 sm:px-3">
        <div className="flex items-center gap-1.5 min-w-0 mr-1.5">
          {onToggleSidebar && (
            <button
              type="button"
              onClick={onToggleSidebar}
              className="flex h-7 w-7 items-center justify-center rounded-md border border-[#30363d] bg-[#161b22] text-[#8b949e] hover:text-[#e6edf3] transition-colors shrink-0"
              title={sidebarOpen ? "Hide sidebar (Ctrl+B)" : "Show sidebar (Ctrl+B)"}
            >
              <PanelLeft size={13} />
            </button>
          )}
          <div className="min-w-0 max-w-[56px] sm:max-w-[180px] truncate">
            <h1 className="truncate text-xs font-semibold text-[#e6edf3]">{sessionTitle || 'New Chat'}</h1>
            <span className="text-[10px] text-[#6e7681] whitespace-nowrap block">{messages.length} messages</span>
          </div>
        </div>

        {/* Action toolbar - compact, responsive, strictly non-overlapping */}
        <div className="flex items-center gap-1 sm:gap-1.5 shrink-0">
          {/* Live Shell button (icon only) */}
          {onOpenLiveShell && (
            <button
              type="button"
              onClick={onOpenLiveShell}
              className="flex h-7 w-7 items-center justify-center rounded-md border border-[#30363d] bg-[#161b22] text-[#8b949e] hover:border-[#3fb950]/50 hover:text-[#e6edf3] transition-colors shrink-0"
              title="Open Live Shell terminal"
            >
              <Terminal size={13} className="text-[#3fb950]" />
            </button>
          )}

          {/* Council Mode Toggle */}
          <button
            type="button"
            onClick={() => setCouncilMode((v) => !v)}
            disabled={!councilSecondCandidate}
            className={`flex h-7 items-center gap-1 rounded-md border px-2 text-xs transition-colors shrink-0 disabled:cursor-not-allowed disabled:opacity-40 ${
              councilMode
                ? 'border-[#f0883e] bg-[#f0883e]/15 text-[#f0883e] font-medium'
                : 'border-[#30363d] bg-[#161b22] text-[#8b949e] hover:text-[#e6edf3]'
            }`}
            title={councilSecondCandidate ? 'Council Mode (Dual Model A/B Test)' : 'Council Mode needs at least two configured models'}
          >
            <Users size={13} />
            <span className="hidden md:inline text-xs">Council</span>
          </button>

          {/* Model Picker */}
          <button
            type="button"
            onClick={onOpenModelPicker}
            className="flex h-7 max-w-[58px] sm:max-w-[145px] items-center gap-1 rounded-md border border-[#30363d] bg-[#161b22] px-2 text-xs text-[#58a6ff] hover:bg-[#21262d] transition-colors shrink-0"
            title="Choose model and reasoning"
          >
            <span className="truncate font-mono text-[11px]">{effectiveConfig?.provider.model || 'Select model'}</span>
            {effectiveConfig?.thinking && <Brain size={12} className="shrink-0 text-[#8b949e]" />}
          </button>

          {/* Task / Artifacts Bubble Button */}
          {(() => {
            const runningTasks = subagents?.filter((s) => s.status === 'running').length || 0;
            return (
              <button
                type="button"
                onClick={onToggleArtifacts}
                disabled={artifactCount === 0 && runningTasks === 0}
                className={`flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-xs transition-all shrink-0 ${
                  isArtifactOpen
                    ? 'border-[#58a6ff] bg-[#58a6ff]/20 text-[#58a6ff] font-medium shadow-sm'
                    : runningTasks > 0
                    ? 'border-[#f0883e]/60 bg-[#f0883e]/15 text-[#f0883e] font-medium animate-pulse hover:bg-[#f0883e]/25'
                    : 'border-[#30363d] bg-[#161b22] text-[#8b949e] hover:text-[#e6edf3]'
                } disabled:cursor-not-allowed disabled:opacity-40 hover:disabled:text-[#8b949e]`}
                title={
                  isArtifactOpen
                    ? 'Close task inspector / artifacts panel'
                    : runningTasks > 0
                    ? `${runningTasks} task(s) active - click to inspect`
                    : 'Show task inspector / artifacts panel'
                }
              >
                {runningTasks > 0 ? (
                  <>
                    <span className="relative flex h-2 w-2">
                      <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[#f0883e] opacity-75" />
                      <span className="relative inline-flex rounded-full h-2 w-2 bg-[#f0883e]" />
                    </span>
                    <span className="text-[11px] font-mono font-semibold text-[#f0883e]">{runningTasks} Active</span>
                  </>
                ) : (
                  <>
                    <PanelRight size={13} />
                    {artifactCount > 0 && (
                      <span className="rounded bg-[#1f6feb] px-1 font-mono text-[9px] font-bold text-white">
                        {artifactCount}
                      </span>
                    )}
                  </>
                )}
              </button>
            );
          })()}

          {/* More Actions Dropdown Menu (...) */}
          <div className="relative shrink-0" data-more-menu>
            <button
              type="button"
              onClick={() => setMoreMenuOpen((v) => !v)}
              className={`flex h-7 w-7 items-center justify-center rounded-md border text-xs transition-colors ${
                moreMenuOpen
                  ? 'border-[#58a6ff] bg-[#161b22] text-[#e6edf3]'
                  : 'border-[#30363d] bg-[#161b22] text-[#8b949e] hover:text-[#e6edf3]'
              }`}
              title="More actions (Search, Export, Settings)"
            >
              <MoreHorizontal size={14} />
            </button>
            {moreMenuOpen && (
              <div className="absolute right-0 top-full mt-1.5 z-50 w-52 rounded-xl border border-[#30363d] bg-[#161b22] p-1 shadow-2xl">
                {onOpenSpotlight && (
                  <button
                    type="button"
                    onClick={() => { setMoreMenuOpen(false); onOpenSpotlight(); }}
                    className="flex w-full items-center justify-between rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                  >
                    <span className="flex items-center gap-2">
                      <Search size={13} className="text-[#8b949e]" />
                      Search
                    </span>
                    <kbd className="rounded border border-[#30363d] bg-[#0d1117] px-1 text-[9px] text-[#6e7681]">Alt+Space</kbd>
                  </button>
                )}
                {onExportSession && (
                  <>
                    <div className="my-1 border-t border-[#30363d]/60 px-2 py-0.5 text-[9px] font-semibold uppercase tracking-wider text-[#6e7681]">Export</div>
                    <button
                      type="button"
                      onClick={() => { setMoreMenuOpen(false); onExportSession('html'); }}
                      className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <Download size={13} className="text-[#58a6ff]" />
                      <span>HTML (PDF-ready)</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => { setMoreMenuOpen(false); onExportSession('markdown'); }}
                      className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <Download size={13} className="text-[#3fb950]" />
                      <span>Markdown (.md)</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => { setMoreMenuOpen(false); onExportSession('json'); }}
                      className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <Download size={13} className="text-[#d29922]" />
                      <span>JSON transcript</span>
                    </button>
                  </>
                )}
                <div className="my-1 border-t border-[#30363d]/60" />
                <button
                  type="button"
                  onClick={() => { setMoreMenuOpen(false); onOpenSettings(); }}
                  className="flex w-full items-center justify-between rounded-lg px-2.5 py-1.5 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                >
                  <span className="flex items-center gap-2">
                    <Settings size={13} className="text-[#8b949e]" />
                    Settings
                  </span>
                  <kbd className="rounded border border-[#30363d] bg-[#0d1117] px-1 text-[9px] text-[#6e7681]">Ctrl+,</kbd>
                </button>
                <button
                  type="button"
                  onClick={() => { setMoreMenuOpen(false); onClearSession(); }}
                  className="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-xs text-[#f85149] hover:bg-[#f85149]/15"
                >
                  <Trash2 size={13} />
                  <span>Clear conversation</span>
                </button>
              </div>
            )}
          </div>
        </div>
      </header>

      <div
        ref={parentRef}
        className="min-h-0 flex-1 overflow-y-auto px-4 py-5"
        onWheel={(e) => {
          if (e.deltaY < 0) {
            // User intentionally scrolled up with the mouse wheel
            stickToBottomRef.current = false;
          }
        }}
        onTouchMove={() => {
          const element = parentRef.current;
          if (element) {
            const distanceToBottom = element.scrollHeight - element.scrollTop - element.clientHeight;
            if (distanceToBottom > 35) {
              stickToBottomRef.current = false;
            }
          }
        }}
        onScroll={(event) => {
          const element = event.currentTarget;
          const hasOverflow = element.scrollHeight > element.clientHeight + 40;
          const distanceToBottom = element.scrollHeight - element.scrollTop - element.clientHeight;
          const isAtBottom = !hasOverflow || distanceToBottom < 35;
          if (isAtBottom) {
            stickToBottomRef.current = true;
            setShowJumpBottom(false);
          } else {
            stickToBottomRef.current = false;
            setShowJumpBottom(hasOverflow);
          }
        }}
      >
        {messages.length === 0 ? (
          <div className="mx-auto flex h-full max-w-xl flex-col items-center justify-center text-center">
            <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-2xl border border-[#30363d] bg-[#161b22] shadow-sm">
              <AetherLogo size={24} />
            </div>
            <h2 className="text-lg font-semibold text-[#e6edf3]">Build with AetherAgent</h2>
            <p className="mt-2 text-xs leading-5 text-[#8b949e]">Ask about the active workspace, attach context with <span className="text-[#58a6ff]">@</span>, or use <span className="text-[#58a6ff]">/</span> for commands.</p>
            <div className="mt-6 grid w-full grid-cols-1 gap-2 text-left sm:grid-cols-2">
              <div className="rounded-lg border border-[#30363d] bg-[#161b22] p-3 text-xs text-[#8b949e]"><span className="mb-1 block font-medium text-[#e6edf3]">Native workspace tools</span>Read, edit, search, run PowerShell, and inspect diffs.</div>
              <div className="rounded-lg border border-[#30363d] bg-[#161b22] p-3 text-xs text-[#8b949e]"><span className="mb-1 block font-medium text-[#e6edf3]">Provider flexibility</span>OpenAI-compatible, Anthropic, DeepSeek, and Ollama.</div>
            </div>
          </div>
        ) : (
          <div className="relative mx-auto w-full max-w-3xl" style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const message = messages[virtualRow.index];
              if (!message) return null;
              return (
                <div
                  key={virtualRow.key}
                  data-index={virtualRow.index}
                  ref={virtualizer.measureElement}
                  className="absolute left-0 top-0 w-full"
                  style={{ transform: `translateY(${virtualRow.start}px)` }}
                >
                  <MessageRow
                    message={message}
                    onSelectArtifact={onSelectArtifact}
                    onSelectCouncilWinner={handleSelectCouncilWinner}
                    onRetry={onRetryTurn ? () => void onRetryTurn(message.id) : undefined}
                    onEditAndResend={
                      onEditAndResendMessage
                        ? (newContent) => void onEditAndResendMessage(message.id, newContent)
                        : undefined
                    }
                    onRollback={onRollbackToMessage ? () => void onRollbackToMessage(message.id) : undefined}
                    isStreaming={isStreaming}
                  />
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Antigravity Timeline Scrubber */}
      <TimelineScrubber
        messages={messages}
        sessionTitle={sessionTitle}
        scrollElement={parentRef.current}
        onJumpToMessage={handleJumpToMessage}
      />

      {/* Floating Jump to Bottom Button */}
      {showJumpBottom && (
        <button
          type="button"
          onClick={handleJumpToBottom}
          aria-label="Jump to bottom"
          className="absolute bottom-20 right-8 z-30 flex items-center gap-1.5 rounded-full border border-[#30363d] bg-[#161b22]/95 px-3 py-1.5 text-xs text-[#e6edf3] shadow-xl backdrop-blur-md hover:border-[#58a6ff]/60 hover:bg-[#21262d] transition-all animate-fadeIn"
        >
          <ChevronDown size={13} className="text-[#58a6ff]" />
          <span className="font-medium text-[11px]">
            {isStreaming ? 'New output' : 'Jump to bottom'}
          </span>
          {isStreaming && (
            <span className="relative flex h-2 w-2">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-[#f0883e] opacity-75" />
              <span className="relative inline-flex rounded-full h-2 w-2 bg-[#f0883e]" />
            </span>
          )}
        </button>
      )}

      <div className="relative shrink-0 border-t border-[#30363d] bg-[#0d1117] p-3">
        {/* Council Mode Banner with Candidate A and Candidate B selectors */}
        {councilMode && (
          <div className="mx-auto mb-2.5 w-full max-w-3xl rounded-lg border border-[#30363d] bg-[#161b22] p-2.5 shadow-sm">
            <div className="flex items-center justify-between border-b border-[#21262d] pb-2 text-xs">
              <div className="flex items-center gap-2">
                <span className="h-1.5 w-1.5 rounded-full bg-[#f0883e]" />
                <span className="font-mono text-[11px] font-medium text-[#e6edf3]">Dual Model Comparison</span>
                <span className="text-[10px] text-[#8b949e]">(A/B)</span>
              </div>
              <button
                type="button"
                onClick={() => setCouncilMode(false)}
                className="text-[11px] text-[#8b949e] hover:text-[#e6edf3] transition-colors"
                title="Close comparison mode"
              >
                Close
              </button>
            </div>

            <div className="mt-2 flex flex-col sm:flex-row items-stretch sm:items-center gap-2">
              {/* Candidate A Picker */}
              <div className="flex flex-1 items-center gap-2 rounded border border-[#30363d] bg-[#0d1117] px-2.5 py-1.5">
                <span className="font-mono text-[10px] font-bold text-[#58a6ff]">A</span>
                <select
                  value={councilModelA ? `${councilModelA.providerId}::${councilModelA.model}` : ''}
                  onChange={(e) => {
                    const [pId, ...mParts] = e.target.value.split('::');
                    setCouncilModelA({ providerId: pId, model: mParts.join('::') });
                  }}
                  className="w-full bg-transparent font-mono text-xs text-[#e6edf3] focus:outline-none cursor-pointer"
                >
                  {availableModelsList.map((m) => (
                    <option key={`a-${m.providerId}-${m.model}`} value={`${m.providerId}::${m.model}`} className="bg-[#161b22] text-[#e6edf3]">
                      {m.model} ({m.providerName})
                    </option>
                  ))}
                </select>
              </div>

              <span className="text-center font-mono text-[11px] text-[#6e7681] px-1 select-none">vs</span>

              {/* Candidate B Picker */}
              <div className="flex flex-1 items-center gap-2 rounded border border-[#30363d] bg-[#0d1117] px-2.5 py-1.5">
                <span className="font-mono text-[10px] font-bold text-[#d29922]">B</span>
                <select
                  value={councilModelB ? `${councilModelB.providerId}::${councilModelB.model}` : ''}
                  onChange={(e) => {
                    const [pId, ...mParts] = e.target.value.split('::');
                    setCouncilModelB({ providerId: pId, model: mParts.join('::') });
                  }}
                  className="w-full bg-transparent font-mono text-xs text-[#e6edf3] focus:outline-none cursor-pointer"
                >
                  {availableModelsList.map((m) => (
                    <option key={`b-${m.providerId}-${m.model}`} value={`${m.providerId}::${m.model}`} className="bg-[#161b22] text-[#e6edf3]">
                      {m.model} ({m.providerName})
                    </option>
                  ))}
                </select>
              </div>
            </div>
          </div>
        )}

        {subagents && subagents.length > 0 && (
          <SubagentTreeCard subagents={subagents} onSelectArtifact={onSelectArtifact} />
        )}

        {backgroundTasks && backgroundTasks.length > 0 && (
          <BackgroundTasksCard
            tasks={backgroundTasks}
            onKillTask={onKillBackgroundTask}
            onRefreshTasks={onRefreshBackgroundTasks}
          />
        )}

        {activeSkills.length > 0 && isStreaming && (
          <div className="mx-auto mb-2 flex max-w-3xl flex-wrap items-center gap-1.5 animate-fadeIn">
            {activeSkills.map((sk) => (
              <div
                key={sk.name}
                className="inline-flex items-center gap-1.5 rounded-full border border-[#3fb950]/30 bg-[#3fb950]/10 px-2.5 py-0.5 text-[11px] font-medium text-[#3fb950]"
                title={sk.description}
              >
                <BookOpen size={11} />
                <span>Active Skill: {sk.name}</span>
              </div>
            ))}
          </div>
        )}

        {slashOpen && (
          <div className="absolute bottom-[calc(100%-4px)] left-1/2 z-40 w-full max-w-3xl -translate-x-1/2 overflow-hidden rounded-xl border border-[#30363d] bg-[#161b22]/98 shadow-2xl backdrop-blur-md">
            {/* Active item usage tooltip bubble (Hermes-style) */}
            {slashMatches[activeIndex] && (slashMatches[activeIndex].usage || slashMatches[activeIndex].description) && (
              <div className="hidden lg:flex absolute right-3 top-2.5 z-50 max-w-xs flex-col rounded-lg border border-[#30363d] bg-[#0d1117]/95 p-2 text-xs shadow-2xl backdrop-blur-md">
                <span className="font-mono text-[11px] font-bold text-[#f0883e]">
                  {slashMatches[activeIndex].command}
                </span>
                <span className="mt-0.5 text-[11px] text-[#8b949e] whitespace-pre-line">
                  {slashMatches[activeIndex].usage || slashMatches[activeIndex].description}
                </span>
              </div>
            )}

            <div className="max-h-80 overflow-y-auto p-1.5 scrollbar-thin">
              {groupedMatches.map((group) => (
                <div key={group.category} className="mb-1">
                  <div className="px-2.5 py-1 text-[10px] font-bold uppercase tracking-wider text-[#8b949e]">
                    {group.category}
                  </div>
                  {group.items.map((item) => {
                    const flatIndex = slashMatches.indexOf(item);
                    const isActive = flatIndex === activeIndex;
                    return (
                      <button
                        key={item.id}
                        type="button"
                        data-slash-item={flatIndex}
                        onClick={() => handleSelectSlashItem(item)}
                        onMouseEnter={() => setActiveIndex(flatIndex)}
                        className={`flex w-full items-center justify-between gap-3 rounded-lg px-2.5 py-1.5 text-left text-xs transition-colors ${
                          isActive
                            ? 'border border-[#f0883e]/30 bg-[#382618] text-white shadow-sm'
                            : 'text-[#c9d1d9] hover:bg-[#21262d]'
                        }`}
                      >
                        <div className="flex min-w-0 items-center gap-2">
                          {item.iconType === 'terminal' ? (
                            <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded border border-[#f0883e]/50 bg-[#f0883e]/15 font-mono text-[9px] font-bold text-[#f0883e]">
                              &gt;_
                            </span>
                          ) : (
                            <span className="flex h-4 w-4 shrink-0 items-center justify-center text-[#f0883e]">
                              <BookOpen size={13} className="shrink-0 text-[#f0883e]" />
                            </span>
                          )}
                          <span className={`truncate font-mono font-semibold ${isActive ? 'text-[#f0883e]' : 'text-[#f0f6fc]'}`}>
                            {item.command}
                          </span>
                        </div>
                        <div className="hidden shrink-0 items-center text-right text-xs text-[#8b949e] sm:flex">
                          {item.iconType === 'skill' && (
                            <span className="mr-1 font-mono text-[9px] text-[#f0883e]/80">skill</span>
                          )}
                          <span className="max-w-[280px] truncate sm:max-w-[400px]">{item.description}</span>
                        </div>
                      </button>
                    );
                  })}
                </div>
              ))}
            </div>
          </div>
        )}
        {mentionOpen && (
          <div className="absolute bottom-[calc(100%-4px)] left-1/2 z-20 max-h-64 w-full max-w-3xl -translate-x-1/2 overflow-y-auto rounded-lg border border-[#30363d] bg-[#161b22] p-1.5 shadow-2xl">
            <div className="flex items-center gap-1.5 px-2 py-1 text-[10px] uppercase tracking-wider text-[#8b949e]"><AtSign size={11} /> Workspace files</div>
            {loadingFiles && <div className="px-3 py-3 text-xs text-[#8b949e]">Searching workspace…</div>}
            {!loadingFiles && fileOptions.length === 0 && <div className="px-3 py-3 text-xs text-[#8b949e]">No matching files</div>}
            {fileOptions.map((path, index) => (
              <button key={path} type="button" onClick={() => selectMention(path)} onMouseEnter={() => setActiveIndex(index)} className={`flex w-full items-center gap-2 rounded px-3 py-2 text-left font-mono text-[11px] ${index === activeIndex ? 'bg-[#21262d] text-[#e6edf3]' : 'text-[#8b949e]'}`}>
                <File size={12} className="shrink-0" /><span className="truncate">{path}</span>
              </button>
            ))}
          </div>
        )}
        {/* Queued Prompts Banner */}
        {queuedPrompts.length > 0 && (
          <div
            className="mx-auto mb-2.5 w-full max-w-3xl space-y-1.5 animate-fadeIn"
            data-testid="queued-prompts-banner"
          >
            <div className="flex items-center justify-between px-1 text-[11px] font-medium text-[#8b949e]">
              <span className="flex items-center gap-1.5 font-mono text-[10px] text-[#f0883e]">
                <Clock size={11} className={isStreaming ? 'animate-pulse' : ''} />
                Queued Instructions ({queuedPrompts.length})
              </span>
              {stoppedWithQueue && !isStreaming && (
                <button
                  type="button"
                  onClick={handleResumeQueue}
                  className="flex items-center gap-1 rounded bg-[#238636] px-2 py-0.5 text-[10px] font-medium text-white hover:bg-[#2ea043] transition-colors"
                >
                  <Play size={10} fill="currentColor" /> Resume Queue
                </button>
              )}
            </div>
            {queuedPrompts.map((item, index) => (
              <div
                key={item.id}
                className="flex items-center justify-between gap-2 rounded-xl border border-[#30363d] bg-[#161b22]/95 px-3 py-1.5 text-xs text-[#e6edf3] shadow-md backdrop-blur-sm transition-all hover:border-[#58a6ff]/40"
                data-testid={`queued-prompt-${index}`}
              >
                <div className="flex items-center gap-2 min-w-0 flex-1">
                  <span className="shrink-0 rounded bg-[#30363d]/70 px-1.5 py-0.5 font-mono text-[10px] text-[#f0883e]">
                    #{index + 1}
                  </span>
                  <p className="truncate text-xs text-[#e6edf3]" title={item.prompt}>
                    {item.prompt}
                  </p>
                  {item.attachments && item.attachments.length > 0 && (
                    <span className="shrink-0 flex items-center gap-1 rounded bg-[#21262d] px-1.5 py-0.5 text-[10px] font-mono text-[#58a6ff]">
                      <Paperclip size={10} /> +{item.attachments.length}
                    </span>
                  )}
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  <button
                    type="button"
                    onClick={() => handleEditQueued(item.id)}
                    className="rounded px-2 py-0.5 text-[11px] font-medium text-[#8b949e] hover:bg-[#21262d] hover:text-[#58a6ff] transition-colors"
                    title="Edit prompt in composer"
                  >
                    Edit
                  </button>
                  <button
                    type="button"
                    onClick={() => handleRemoveQueued(item.id)}
                    className="rounded p-1 text-[#8b949e] hover:bg-[#21262d] hover:text-[#f85149] transition-colors"
                    aria-label="Remove queued prompt"
                    title="Remove from queue"
                  >
                    <X size={12} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        <div className="mx-auto max-w-3xl rounded-2xl border border-[#30363d] bg-[#161b22] shadow-sm transition-colors focus-within:border-[#58a6ff]/70 focus-within:ring-1 focus-within:ring-[#58a6ff]/20">
          {attachedContexts.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5 border-b border-[#30363d]/50 px-3 py-2">
              {attachedContexts.map((att) => (
                <span
                  key={att.id}
                  className="inline-flex items-center gap-1.5 rounded-md border border-[#30363d] bg-[#0d1117] px-2 py-0.5 text-[11px] font-mono text-[#58a6ff]"
                >
                  {att.isDir ? (
                    <Folder size={11} className="text-[#d29922]" />
                  ) : /\.(png|jpe?g|gif|webp|svg|bmp)$/i.test(att.name) ? (
                    <ImageIcon size={11} className="text-[#a371f7]" />
                  ) : (
                    <Paperclip size={11} className="text-[#58a6ff]" />
                  )}
                  <span className="max-w-[200px] truncate" title={att.path}>{att.name}</span>
                  <button
                    type="button"
                    onClick={() => setAttachedContexts((prev) => prev.filter((item) => item.id !== att.id))}
                    className="text-[#8b949e] hover:text-[#f85149]"
                    aria-label={`Remove ${att.name}`}
                  >
                    <X size={11} />
                  </button>
                </span>
              ))}
            </div>
          )}
          <div className="flex items-start">
            {input.startsWith('/') && (
              <div className="flex shrink-0 select-none items-center gap-1 pl-3.5 pt-3.5 font-mono text-xs font-bold text-[#f0883e]" title="Command mode">
                &gt;_
              </div>
            )}
            <textarea
              ref={textareaRef}
              value={input}
              rows={2}
              onChange={(event) => {
                setInput(event.target.value);
                setCursor(event.target.selectionStart);
                setMenuDismissed(false);
              }}
              onSelect={(event) => setCursor(event.currentTarget.selectionStart)}
              onKeyDown={onKeyDown}
              onPaste={handlePaste}
              placeholder="Message AetherAgent…"
              className={`max-h-44 min-h-[58px] w-full resize-none border-0 bg-transparent pb-2 pt-3.5 text-[13px] leading-5 text-[#e6edf3] placeholder:text-[#6e7681] focus:border-0 focus:outline-none focus:ring-0 focus-visible:outline-none ${
                input.startsWith('/') ? 'pl-2 pr-4' : 'px-4'
              }`}
              aria-label="Message AetherAgent"
            />
          </div>
          <div className="flex items-center justify-between gap-3 px-3 pb-2.5 pt-0.5">
            {/* Left cluster: [+] and [Model / Thinking pill] side-by-side */}
            <div className="flex items-center gap-1.5">
              <div className="relative" data-context-menu>
                <button
                  type="button"
                  onClick={() => {
                    setAddContextOpen((value) => !value);
                    setSkillsMenuOpen(false);
                  }}
                  className="flex h-7 w-7 items-center justify-center rounded-lg border border-[#30363d] bg-[#21262d] text-[#8b949e] transition-colors hover:bg-[#30363d] hover:text-[#e6edf3]"
                  title="Add Context (Files, Folder, Mentions)"
                  aria-label="Add Context"
                >
                  <Plus size={14} />
                </button>
                {addContextOpen && (
                  <div className="absolute bottom-9 left-0 z-30 w-44 rounded-xl border border-[#30363d] bg-[#161b22] p-1 shadow-2xl">
                    <div className="px-2.5 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-[#8b949e]">Add Context</div>
                    <button
                      type="button"
                      onClick={handleAddFiles}
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <Paperclip size={14} className="text-[#58a6ff]" /> Files
                    </button>
                    <button
                      type="button"
                      onClick={handleAddFolder}
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <Folder size={14} className="text-[#d29922]" /> Folder
                    </button>
                    <button
                      type="button"
                      onClick={handleAddMention}
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <AtSign size={14} className="text-[#a371f7]" /> Mentions
                    </button>
                    <button
                      type="button"
                      onClick={handleOpenSkillsList}
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <BookOpen size={14} className="text-[#3fb950]" /> Actions & Skills
                    </button>
                    <button
                      type="button"
                      onClick={handleAddBrowser}
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <Globe size={14} className="text-[#58a6ff]" /> Browser
                    </button>
                  </div>
                )}
                {skillsMenuOpen && (
                  <div className="absolute bottom-9 left-0 z-30 max-h-60 w-64 overflow-y-auto rounded-xl border border-[#30363d] bg-[#161b22] p-1.5 shadow-2xl">
                    <div className="flex items-center justify-between px-2 py-1 text-[10px] font-semibold uppercase tracking-wider text-[#8b949e]">
                      <span>Available Skills</span>
                      <button type="button" onClick={() => setSkillsMenuOpen(false)} className="text-[#8b949e] hover:text-white" aria-label="Close skills">
                        <X size={12} />
                      </button>
                    </div>
                    {availableSkills.length === 0 ? (
                      <div className="px-2 py-3 text-center text-xs text-[#6e7681]">No skills detected</div>
                    ) : (
                      availableSkills.map((sk) => (
                        <button
                          key={sk.name}
                          type="button"
                          onClick={() => handleSelectSkillAction(sk)}
                          className="flex w-full flex-col items-start rounded-lg px-2 py-1.5 text-left hover:bg-[#21262d]"
                        >
                          <span className="font-mono text-xs text-[#58a6ff]">{sk.name}</span>
                          <span className="line-clamp-1 text-[10px] text-[#8b949e]">{sk.description || 'No description'}</span>
                        </button>
                      ))
                    )}
                  </div>
                )}
              </div>

              {/* Voice Dictation (Microphone) */}
              <button
                type="button"
                onClick={toggleListening}
                className={`flex h-7 w-7 items-center justify-center rounded-lg border transition-all ${
                  isListening
                    ? 'border-[#f85149] bg-[#da3633]/25 text-[#f85149] animate-pulse ring-2 ring-[#f85149]/40'
                    : 'border-[#30363d] bg-[#21262d] text-[#8b949e] hover:bg-[#30363d] hover:text-[#e6edf3]'
                }`}
                title={isListening ? 'Listening to voice... Click to stop' : 'Voice Dictation (Speech-to-Text)'}
                aria-label="Voice Dictation"
              >
                {isListening ? <MicOff size={13} className="text-[#f85149]" /> : <Mic size={13} />}
              </button>

              {/* Model & Thinking pill - directly next to [+] like Antigravity & Hermes */}
              <div className="relative" data-thinking-menu>
                <button
                  type="button"
                  onClick={() => {
                    setThinkingMenuOpen((value) => !value);
                    setAddContextOpen(false);
                    setSkillsMenuOpen(false);
                  }}
                  className="flex h-7 items-center gap-1.5 rounded-lg border border-[#30363d] bg-[#21262d] px-2.5 text-xs text-[#e6edf3] transition-colors hover:bg-[#30363d]"
                  title="Model & Thinking controls"
                >
                  <SlidersHorizontal size={12} className="shrink-0 text-[#58a6ff]" />
                  <span className="max-w-[130px] truncate font-mono text-[11px] font-medium sm:max-w-[180px]">
                    {effectiveConfig?.provider.model || 'Select model'}
                  </span>
                  <span
                    className={`rounded px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider ${
                      effectiveConfig?.thinking
                        ? 'bg-[#1f6feb]/25 text-[#58a6ff]'
                        : 'bg-[#30363d] text-[#8b949e]'
                    }`}
                  >
                    {effectiveConfig?.thinking
                      ? (effectiveConfig.reasoningEffort ? effectiveConfig.reasoningEffort.replace('_', ' ') : 'On')
                      : 'Off'}
                  </span>
                  <ChevronUp size={12} className={`text-[#8b949e] transition-transform ${thinkingMenuOpen ? 'rotate-180' : ''}`} />
                </button>

              {thinkingMenuOpen && (
                <div className="fixed bottom-24 left-3 right-3 z-50 w-auto rounded-xl border border-[#30363d] bg-[#161b22] p-2.5 shadow-2xl sm:absolute sm:bottom-9 sm:left-0 sm:right-auto sm:w-72">
                  <div className="mb-2.5 flex items-center justify-between border-b border-[#30363d] pb-2">
                    <div className="flex items-center gap-1.5">
                      <Brain size={14} className={effectiveConfig?.thinking ? 'text-[#58a6ff]' : 'text-[#8b949e]'} />
                      <span className="text-xs font-semibold text-[#e6edf3]">Thinking Process</span>
                    </div>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={effectiveConfig?.thinking}
                      onClick={() => onUpdateOverride?.({ thinking: !effectiveConfig?.thinking })}
                      className={`relative h-4 w-8 rounded-full transition-colors ${
                        effectiveConfig?.thinking ? 'bg-[#1f6feb]' : 'bg-[#30363d]'
                      }`}
                      title="Toggle thinking"
                    >
                      <span
                        className={`absolute left-0.5 top-0.5 h-3 w-3 rounded-full bg-white transition-transform ${
                          effectiveConfig?.thinking ? 'translate-x-4' : 'translate-x-0'
                        }`}
                      />
                    </button>
                  </div>

                  <div className="mb-3 space-y-1.5">
                    <div className="flex items-center justify-between">
                      <span className="text-[10px] font-semibold uppercase tracking-wider text-[#8b949e]">
                        Reasoning Effort
                      </span>
                      <span className="font-mono text-[10px] text-[#58a6ff] capitalize">
                        {effectiveConfig?.thinking ? effectiveConfig?.reasoningEffort?.replace('_', ' ') : 'Disabled'}
                      </span>
                    </div>
                    <div className="grid grid-cols-3 gap-1">
                      {(['minimal', 'low', 'medium', 'high', 'extra_high', 'max'] as const).map((effort) => {
                        const isEffortActive = effectiveConfig?.thinking && effectiveConfig?.reasoningEffort === effort;
                        return (
                          <button
                            key={effort}
                            type="button"
                            disabled={!effectiveConfig?.thinking}
                            onClick={() => onUpdateOverride?.({ reasoningEffort: effort })}
                            className={`rounded border px-1.5 py-1 text-center font-mono text-[10px] capitalize transition-colors ${
                              !effectiveConfig?.thinking
                                ? 'border-transparent bg-[#0d1117] text-[#6e7681] opacity-40 cursor-not-allowed'
                                : isEffortActive
                                ? 'border-[#58a6ff] bg-[#1f6feb]/25 font-semibold text-[#58a6ff]'
                                : 'border-[#30363d] bg-[#0d1117] text-[#8b949e] hover:bg-[#21262d] hover:text-[#e6edf3]'
                            }`}
                          >
                            {effort.replace('_', ' ')}
                          </button>
                        );
                      })}
                    </div>
                  </div>

                  <div className="border-t border-[#30363d] pt-2">
                    <div className="mb-1.5 flex items-center justify-between">
                      <span className="text-[10px] font-semibold uppercase tracking-wider text-[#8b949e]">
                        Quick Model Switch
                      </span>
                      <button
                        type="button"
                        onClick={() => {
                          setThinkingMenuOpen(false);
                          onOpenModelPicker();
                        }}
                        className="text-[10px] font-medium text-[#58a6ff] hover:underline"
                      >
                        All models…
                      </button>
                    </div>
                    <div className="max-h-36 space-y-0.5 overflow-y-auto">
                      {recentModels.map((item) => {
                        const isCur = item.providerId === effectiveConfig?.provider.id && item.model === effectiveConfig?.provider.model;
                        return (
                          <button
                            key={item.key}
                            type="button"
                            onClick={() => {
                              onSelectModel?.(item.providerId, item.model);
                              setThinkingMenuOpen(false);
                            }}
                            className={`flex w-full items-center justify-between rounded px-2 py-1 text-left text-xs transition-colors ${
                              isCur
                                ? 'bg-[#1f6feb]/20 font-medium text-[#58a6ff]'
                                : 'text-[#8b949e] hover:bg-[#21262d] hover:text-[#e6edf3]'
                            }`}
                          >
                            <span className="truncate font-mono text-[11px]">{item.model}</span>
                            <span className="ml-1 shrink-0 text-[9px] text-[#6e7681]">{item.providerName}</span>
                          </button>
                        );
                      })}
                    </div>
                  </div>
                </div>
              )}
            </div>
            </div>

            {/* Right cluster: [Enter to send hint] and [Send / Stop button] */}
            <div className="flex items-center gap-3">
              
              {isReviewing && <span role="status" className="flex items-center gap-1.5 text-[11px] text-[#8b949e]"><Brain size={12} /> Reviewing…</span>}
              <div className="shrink-0">
                {isStreaming ? (
                  <div className="flex items-center gap-2">
                    {((input || '').trim().length > 0 || attachedContexts.length > 0) && (
                      <button
                        type="button"
                        onClick={send}
                        className="flex items-center gap-1.5 rounded-lg border border-[#30363d] bg-[#21262d] px-2.5 py-1.5 text-xs font-medium text-[#58a6ff] hover:bg-[#30363d] hover:border-[#58a6ff]/50 transition-colors"
                        title="Add to queue (executes after current turn)"
                        aria-label="Queue message"
                      >
                        <Clock size={12} className="text-[#f0883e]" />
                        <span>Queue</span>
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={onStop}
                      disabled={stopRequested}
                      className="flex items-center gap-1.5 rounded-lg bg-[#da3633] px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-[#f85149] disabled:opacity-50"
                    >
                      <Square size={12} fill="currentColor" /> {stopRequested ? 'Stopping…' : 'Stop'}
                    </button>
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={send}
                    disabled={isReviewing || (!(input || '').trim() && attachedContexts.length === 0)}
                    className="rounded-lg bg-[#1f6feb] p-2 text-white transition-colors hover:bg-[#388bfd] disabled:cursor-not-allowed disabled:opacity-40"
                    aria-label="Send message"
                  >
                    <Send size={15} />
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>
      {input && (
        <button type="button" onClick={() => { setInput(''); setMenuDismissed(false); textareaRef.current?.focus(); }} className="sr-only" aria-label="Clear composer"><X /></button>
      )}
    </div>
  );
}
