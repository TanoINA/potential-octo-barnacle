import { RunStatusBar } from './RunStatusBar';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  LoaderCircle,
  ArrowRight,
  Pencil,
  AtSign,
  ChevronDown,
  BookOpen,
  Brain,
  ChevronUp,
  Clock,
  File,
  FileCode2,
  Folder,
  Globe,
  Image as ImageIcon,
  Mic,
  MicOff,
  Paperclip,
  Play,
  Plus,
  Search,
  Send,
  SlidersHorizontal,
  Square,
  Trash2,
  X,
  Eye,
  Lock,
  Shield,
  ShieldAlert,
  Check,
} from 'lucide-react';
import { open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import type { AttachedContext, BackgroundTask, ChatMessage, EffectiveRunConfig, SkillInfo, SubagentProgress, UndoTurnResult, ExecutionMode } from '../types/agent';
import { MessageRow } from './MessageRow';
import { AttachmentPreview } from './AttachmentPreview';
import { ActivityTray } from './ActivityTray';
import { SkillPicker } from './SkillPicker';
import { TimelineScrubber } from './TimelineScrubber';
import { ChatHeader } from './ChatHeader';
import { useSpeechRecognition } from '../hooks/useSpeechRecognition';

const EXECUTION_MODES: Array<{
  id: ExecutionMode;
  title: string;
  description: string;
  icon: typeof ShieldAlert;
}> = [
  {
    id: 'request_review',
    title: 'Request Review',
    description: 'Write, command, and MCP tools are blocked.',
    icon: Eye,
  },
  {
    id: 'default',
    title: 'Default',
    description: 'Ask before sensitive operations.',
    icon: Lock,
  },
  {
    id: 'auto',
    title: 'Auto (Preview)',
    description: 'Automatically decide if commands run.',
    icon: Shield,
  },
  {
    id: 'turbo',
    title: 'Turbo',
    description: 'Run all commands without restrictions.',
    icon: ShieldAlert,
  },
];

interface ChatCanvasProps {
  messages: import('../types/agent').ChatMessage[];
  isSessionLoading?: boolean;
  loadingOlderMessages?: boolean;
  onLoadOlderMessages?: () => Promise<void>;
  onLoadLatestMessages?: () => void | Promise<void>;
  hasOlderMessages?: boolean;
  totalMessageCount?: number;
  historyError?: string | null;
  isHistoryPreview?: boolean;
  jumpToMessage?: { messageId: string; request: number } | null;
  runStatus?: import('../hooks/useAgentLiveSession').RunStatus | null;
  isStreaming: boolean;
  stopRequested: boolean;
  onSendMessage: (
    text: string,
    attachments?: AttachedContext[],
    options?: {
      council?: boolean;
      candidateA?: { provider: import('../types/agent').ProviderConfig; thinking?: boolean; reasoningEffort?: import('../types/agent').ReasoningEffort };
      candidateB?: { provider: import('../types/agent').ProviderConfig; thinking?: boolean; reasoningEffort?: import('../types/agent').ReasoningEffort };
      executionMode?: ExecutionMode;
      imageInputs?: import('../types/agent').ImageInput[];
      selectedSkills?: string[];
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
  sessionId?: string;
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
  workspaceCwd?: string;
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
  { id: 'cmd_search', category: 'COMMANDS', command: '/search', description: 'Search the web for up-to-date documentation, news, or solutions', usage: 'Search web\n/search <query>', iconType: 'terminal', needsPrompt: true },
  { id: 'cmd_web', category: 'COMMANDS', command: '/web', description: 'Search the web for up-to-date documentation, news, or solutions', usage: 'Search web\n/web <query>', iconType: 'terminal', needsPrompt: true },
  { id: 'cmd_plan', category: 'COMMANDS', command: '/plan', description: 'Formulate an interactive step-by-step execution plan', usage: 'Formulate execution plan\n/plan <task goal>', iconType: 'terminal', needsPrompt: true },
  { id: 'cmd_goal', category: 'COMMANDS', command: '/goal', description: 'Autonomous goal execution loop until build/test target passes', usage: 'Autonomous goal mode\n/goal <verification target>', iconType: 'terminal', needsPrompt: true },
  { id: 'cmd_grill_me', category: 'COMMANDS', command: '/grill-me', description: 'Reverse-interview mode: proactively grill on edge cases and requirements', usage: 'Reverse-interview mode\n/grill-me [topic]', iconType: 'terminal' },
  { id: 'cmd_tasks', category: 'COMMANDS', command: '/tasks', description: 'List and inspect active background tasks and processes', usage: 'Manage background tasks\n/tasks', iconType: 'terminal' },
  { id: 'cmd_export', category: 'COMMANDS', command: '/export', description: 'Export this conversation transcript to Markdown (.md)', usage: 'Export conversation\n/export', iconType: 'terminal' },
  { id: 'cmd_review', category: 'COMMANDS', command: '/review', description: 'Review the full conversation for reusable lessons', iconType: 'terminal' },
  { id: 'cmd_refine', category: 'COMMANDS', command: '/refine', description: 'Review the full conversation (alias of /review)', iconType: 'terminal' },
  { id: 'cmd_reflect', category: 'COMMANDS', command: '/reflect', description: 'Review the full conversation (alias of /review)', iconType: 'terminal' },
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

type RunOptions = NonNullable<Parameters<ChatCanvasProps['onSendMessage']>[2]>;

interface QueuedPrompt {
  id: string;
  prompt: string;
  attachments?: AttachedContext[];
  options: RunOptions;
  sessionId: string | null | undefined;
  sendNow?: boolean;
}

const EMPTY_QUEUED_PROMPTS: QueuedPrompt[] = [];
type QueueSessionId = QueuedPrompt['sessionId'];

export function getModelContextWindow(modelName?: string, customLimit?: number): number {
  if (customLimit && customLimit > 0) return customLimit;
  if (!modelName) return 128_000;
  const m = modelName.toLowerCase();

  // 1. Explicit numeric size tag anywhere in model name (e.g. -1m, -256k, -32k, -1024k)
  const tagMatch = m.match(/(?:^|[-_./:])(\d+)(k|m)(?:[-_./:]|$)/);
  if (tagMatch) {
    const val = Number(tagMatch[1]);
    if (tagMatch[2] === 'm') return val * 1_000_000;
    if (val >= 8) return val * 1_000;
  }

  // 2. 1M frontier era generations (Gemini, GPT-5/6+, Claude 4/5+, DeepSeek V4+, Kimi K3+, GLM-5+, Qwen 3+, MiniMax M3+, MiMo, Hy4)
  if (/gemini|gpt-[5-9]|claude-(?:fable|.*-[4-9])|deepseek[-_]v[4-9]|kimi-k[3-9]|glm-[5-9]|qwen-?[3-9]|minimax[-_]m[3-9]|mimo|hy[4-9]/i.test(m)) {
    return 1_000_000;
  }

  // 3. Known 200k - 256k generations
  if (/claude-3|claude-(?:sonnet|opus|haiku)|minimax/i.test(m)) return 200_000;
  if (/kimi-k2|codestral|hy3|nemotron-3\.5/i.test(m)) return 256_000;

  // 4. Known 8k / 32k legacy models
  if (/llama-3\b|llama-2/i.test(m) && !/llama-3\.[123]/i.test(m)) return 8_192;
  if (/qwen/i.test(m) && !/qwen-?2\.5/i.test(m)) return 32_768;

  // 5. Standard baseline for modern LLMs (GPT-4o, o1, o3, DeepSeek V3/R1, Qwen 2.5, Llama 3.1+, GLM-4, etc.)
  return 128_000;
}

function estimateSessionTokens(messages: ChatMessage[]): number {
  let charCount = 0;
  let imageTokens = 0;
  for (const msg of messages) {
    if (msg.content) {
      charCount += msg.content.length;
    } else if (msg.segments) {
      for (const seg of msg.segments) {
        if (seg.text) charCount += seg.text.length;
      }
    }
    if (msg.reasoning) charCount += msg.reasoning.length;
    if (msg.preamble) charCount += msg.preamble.length;
    if (msg.imageInputs) imageTokens += msg.imageInputs.length * 1600;

    const toolCalls = msg.toolCalls?.length
      ? msg.toolCalls
      : msg.segments?.flatMap((s) => s.toolCalls ?? []) ?? [];

    for (const tc of toolCalls) {
      if (tc.result) charCount += tc.result.length;
      if (tc.argumentsRaw) charCount += tc.argumentsRaw.length;
      else if (tc.args) charCount += JSON.stringify(tc.args).length;
    }
  }
  return Math.ceil(charCount / 4) + imageTokens;
}

export function ChatCanvas({
  messages,
  isSessionLoading = false,
  loadingOlderMessages = false,
  onLoadOlderMessages,
  onLoadLatestMessages,
  hasOlderMessages = false,
  totalMessageCount = messages.length,
  historyError,
  isHistoryPreview = false,
  jumpToMessage,
  runStatus,
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
  workspaceCwd = '',
}: ChatCanvasProps) {
  const [input, setInput] = useState('');
  const [selectedSkills, setSelectedSkills] = useState<string[]>([]);
  const [cursor, setCursor] = useState(0);
  const [activeIndex, setActiveIndex] = useState(0);
  const [menuDismissed, setMenuDismissed] = useState(false);
  const [councilMode, setCouncilMode] = useState(false);
  const [councilModelA, setCouncilModelA] = useState<{ providerId: string; model: string } | null>(null);
  const [councilModelB, setCouncilModelB] = useState<{ providerId: string; model: string } | null>(null);
  const [sessionQueues, setSessionQueues] = useState(() => new Map<QueueSessionId, QueuedPrompt[]>());
  const [pausedQueues, setPausedQueues] = useState(() => new Set<QueueSessionId>());
  const queuedPrompts = sessionQueues.get(currentSessionId) ?? EMPTY_QUEUED_PROMPTS;
  const stoppedWithQueue = pausedQueues.has(currentSessionId);
  const dispatchingQueueRef = useRef(new Map<QueueSessionId, string>());
  const updateSessionQueue = useCallback((sessionId: QueueSessionId, update: (items: QueuedPrompt[]) => QueuedPrompt[]) => {
    setSessionQueues((previous) => {
      const items = update(previous.get(sessionId) ?? EMPTY_QUEUED_PROMPTS);
      const next = new Map(previous);
      if (items.length) next.set(sessionId, items);
      else next.delete(sessionId);
      return next;
    });
  }, []);
  const setQueuedPrompts = useCallback((update: (items: QueuedPrompt[]) => QueuedPrompt[]) => {
    updateSessionQueue(currentSessionId, update);
  }, [currentSessionId, updateSessionQueue]);
  const pauseSessionQueue = useCallback((sessionId: QueueSessionId, paused: boolean) => {
    setPausedQueues((previous) => {
      if (previous.has(sessionId) === paused) return previous;
      const next = new Set(previous);
      if (paused) next.add(sessionId);
      else next.delete(sessionId);
      return next;
    });
  }, []);
  const setStoppedWithQueue = useCallback((paused: boolean) => {
    pauseSessionQueue(currentSessionId, paused);
  }, [currentSessionId, pauseSessionQueue]);
  const preparedImageInputsRef = useRef(new Map<string, import('../types/agent').ImageInput>());

  const [pendingAttachments, setPendingAttachments] = useState(0);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const attachmentScopeRef = useRef({ sessionId: currentSessionId, workspace: workspaceCwd, generation: 0, pending: 0 });
  const attachmentMountedRef = useRef(true);
  // Update synchronously: a completion must not sneak in before the reset effect.
  if (attachmentScopeRef.current.sessionId !== currentSessionId || attachmentScopeRef.current.workspace !== workspaceCwd) {
    attachmentScopeRef.current = { sessionId: currentSessionId, workspace: workspaceCwd, generation: attachmentScopeRef.current.generation + 1, pending: 0 };
  }
  type AttachmentOrigin = Pick<typeof attachmentScopeRef.current, 'sessionId' | 'workspace' | 'generation'>;
  const isAttachmentOrigin = useCallback((origin: AttachmentOrigin) => {
    const scope = attachmentScopeRef.current;
    return attachmentMountedRef.current && scope.sessionId === origin.sessionId && scope.workspace === origin.workspace && scope.generation === origin.generation;
  }, []);
  const invalidateAttachments = useCallback(() => {
    attachmentScopeRef.current.generation += 1;
    attachmentScopeRef.current.pending = 0;
    setPendingAttachments(0);
    setAttachmentError(null);
  }, []);
  const beginAttachments = useCallback(() => {
    const scope = attachmentScopeRef.current;
    scope.pending += 1;
    setPendingAttachments(scope.pending);
    setAttachmentError(null);
    return { sessionId: scope.sessionId, workspace: scope.workspace, generation: scope.generation };
  }, []);
  const finishAttachments = useCallback((origin: AttachmentOrigin) => {
    if (!isAttachmentOrigin(origin)) return;
    const scope = attachmentScopeRef.current;
    scope.pending = Math.max(0, scope.pending - 1);
    setPendingAttachments(scope.pending);
  }, [isAttachmentOrigin]);
  const reportAttachmentError = useCallback((origin: AttachmentOrigin, error: unknown) => {
    if (isAttachmentOrigin(origin)) setAttachmentError('Could not attach file: ' + String(error));
  }, [isAttachmentOrigin]);
  const appendAttachments = useCallback((origin: AttachmentOrigin, contexts: AttachedContext[]) => {
    if (!isAttachmentOrigin(origin)) return;
    setAttachedContexts((prev) => {
      if (!isAttachmentOrigin(origin)) return prev;
      const existing = new Set(prev.map((context) => context.path));
      return [...prev, ...contexts.filter((context) => {
        if (existing.has(context.path)) return false;
        existing.add(context.path);
        return true;
      })];
    });
  }, [isAttachmentOrigin]);
  useEffect(() => {
    attachmentMountedRef.current = true;
    return () => {
      attachmentMountedRef.current = false;
      attachmentScopeRef.current.generation += 1;
      attachmentScopeRef.current.pending = 0;
    };
  }, []);
  useEffect(() => {
    setAttachedContexts([]);
    setSelectedSkills([]);
    setSkillsMenuOpen(false);
    setLoadingSkills(false);
    setSkillsError(null);
    setAvailableSkills([]);
    preparedImageInputsRef.current.clear();
    setPendingAttachments(0);
    setAttachmentError(null);
    setIsDraggingOver(false);
  }, [currentSessionId, workspaceCwd]);

  // Antrean dan status jeda dimiliki sesi asal, bukan tampilan yang sedang dibuka.
  useEffect(() => {
    if (stopRequested) setStoppedWithQueue(true);
  }, [stopRequested]);

  const queueActionsBlocked = isSessionLoading || isHistoryPreview || isReviewing || runStatus?.phase === 'saving_results'
    || dispatchingQueueRef.current.has(currentSessionId);

  const handleSendQueuedNow = (id: string) => {
    if (queueActionsBlocked || !queuedPrompts.some((item) => item.id === id) || queuedPrompts.some((item) => item.sendNow)) return;
    setStoppedWithQueue(true);
    setQueuedPrompts((items) => {
      const item = items.find((queued) => queued.id === id);
      return item ? [{ ...item, sendNow: true }, ...items.filter((queued) => queued.id !== id)] : items;
    });
    if (isStreaming && !stopRequested) onStop();
  };

  const handleResumeQueue = () => {
    if (!isStreaming && !isReviewing) setStoppedWithQueue(false);
  };

  const handleEditQueued = useCallback((id: string) => {
    const item = queuedPrompts.find((q) => q.id === id);
    if (!item || dispatchingQueueRef.current.has(currentSessionId)) return;
    setQueuedPrompts((prev) => prev.filter((q) => q.id !== id));
    invalidateAttachments();
    setInput(item.prompt);
    setSelectedSkills(item.options.selectedSkills ?? []);
    preparedImageInputsRef.current = new Map((item.options.imageInputs ?? []).map((image) => [image.sourcePath, { ...image }]));
    setAttachedContexts(item.attachments ?? []);
    requestAnimationFrame(() => {
      textareaRef.current?.focus();
    });
  }, [queuedPrompts, currentSessionId, setQueuedPrompts, invalidateAttachments]);

  useEffect(() => {
    const handleInsertPrompt = (e: Event) => {
      const customEvent = e as CustomEvent<{ text: string; autoSend?: boolean }>;
      if (!customEvent.detail?.text) return;
      const { text, autoSend } = customEvent.detail;
      if (autoSend && !isStreaming && !isReviewing && attachmentScopeRef.current.pending === 0) {
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
      invalidateAttachments();
      setInput('');
      setAttachedContexts([]);
      setSelectedSkills([]);
      if (textareaRef.current) {
        textareaRef.current.style.height = 'auto';
      }
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
    if (dispatchingQueueRef.current.has(currentSessionId)) return;
    setQueuedPrompts((prev) => prev.filter((q) => q.id !== id));
  }, [currentSessionId, setQueuedPrompts]);

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
  const previewAttachmentIds = useMemo(() => {
    const ids = new Set<string>();
    for (const attachment of attachedContexts) {
      if (!attachment.isDir && /\.(png|jpe?g|gif|webp)$/i.test(attachment.path)) {
        ids.add(attachment.id);
        if (ids.size === 4) break;
      }
    }
    return ids;
  }, [attachedContexts]);
  useEffect(() => {
    // Removing a file permanently drops its cached bytes, even if re-added later.
    for (const path of preparedImageInputsRef.current.keys()) {
      if (!attachedContexts.some((attachment) => !attachment.isDir && attachment.path === path)) {
        preparedImageInputsRef.current.delete(path);
      }
    }
  }, [attachedContexts]);
  const [addContextOpen, setAddContextOpen] = useState(false);
  const [skillsMenuOpen, setSkillsMenuOpen] = useState(false);
  const [availableSkills, setAvailableSkills] = useState<SkillInfo[]>([]);
  const [loadingSkills, setLoadingSkills] = useState(false);
  const [skillsError, setSkillsError] = useState<string | null>(null);
  const [thinkingMenuOpen, setThinkingMenuOpen] = useState(false);
  const [executionMode, setExecutionMode] = useState<ExecutionMode>(() => {
    try {
      const saved = typeof window !== 'undefined' ? (localStorage.getItem('aether_execution_mode') as ExecutionMode) : null;
      if (saved && ['request_review', 'default', 'auto', 'turbo'].includes(saved)) {
        return saved;
      }
    } catch {}
    return 'turbo';
  });
  const [executionMenuOpen, setExecutionMenuOpen] = useState(false);

  const handleSelectExecutionMode = useCallback((mode: ExecutionMode) => {
    setExecutionMode(mode);
    try {
      localStorage.setItem('aether_execution_mode', mode);
    } catch {}
    setExecutionMenuOpen(false);
  }, []);

  const activeExecutionMode = useMemo(() => {
    return EXECUTION_MODES.find((m) => m.id === executionMode) || EXECUTION_MODES[3];
  }, [executionMode]);
  const ActiveModeIcon = activeExecutionMode.icon;
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

  // The origin is captured before file reads and reused for every native call.
  const prepareAttachmentFiles = useCallback(async (files: File[], origin: AttachmentOrigin, pasted: boolean) => {
    const contexts: AttachedContext[] = [];
    const native = typeof window !== 'undefined' && Boolean((window as any).__TAURI_INTERNALS__);
    for (const file of files) {
      if (!isAttachmentOrigin(origin)) return;
      const osPath = (file as any).path;
      if (native && typeof osPath === 'string' && osPath.trim()) {
        const inspected = await invoke<AttachedContext[]>('inspect_paths', { paths: [osPath] });
        if (!isAttachmentOrigin(origin)) return;
        if (inspected.length) {
          contexts.push(...inspected);
          continue;
        }
      }
      let filename = file.name;
      if (pasted && (!filename || filename === 'image.png' || filename === 'blob')) {
        const ext = file.type.includes('jpeg') ? 'jpg' : file.type.includes('webp') ? 'webp' : 'png';
        filename = 'pasted_image_' + new Date().toISOString().replace(/[^0-9]/g, '').slice(0, 14) + '.' + ext;
      }
      if (native) {
        // Tolak sebelum ArrayBuffer dan array IPC memperbesar penggunaan memori.
        const maxBytes = 25 * 1024 * 1024;
        if (file.size > maxBytes) throw new Error('Attachment exceeds the 25 MiB limit');
        const buffer = await file.arrayBuffer();
        if (!isAttachmentOrigin(origin)) return;
        if (buffer.byteLength > maxBytes) throw new Error('Attachment exceeds the 25 MiB limit');
        const attachment = await invoke<AttachedContext>('save_pasted_attachment', {
          name: filename,
          bytes: Array.from(new Uint8Array(buffer)),
          expectedWorkspace: origin.workspace,
        });
        if (!isAttachmentOrigin(origin)) return;
        contexts.push(attachment);
        continue;
      }
      // Browser-only context placeholders are intentional; native failures never
      // fall back to a filename pretending to be a saved workspace attachment.
      contexts.push({
        id: 'ctx-' + Date.now() + '-' + Math.random().toString(36).slice(2, 6),
        name: filename,
        path: osPath || filename,
        isDir: !pasted && !file.name.includes('.') && file.size === 0,
      });
    }
    appendAttachments(origin, contexts);
  }, [appendAttachments, isAttachmentOrigin]);

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    setIsDraggingOver(false);
    const files = Array.from(e.dataTransfer?.files || []);
    if (!files.length) return;
    const origin = beginAttachments();
    try {
      const paths = files.map((file) => (file as any).path).filter((path): path is string => typeof path === 'string' && Boolean(path.trim()));
      if (paths.length === files.length && (window as any).__TAURI_INTERNALS__) {
        const contexts = await invoke<AttachedContext[]>('inspect_paths', { paths });
        if (!isAttachmentOrigin(origin)) return;
        appendAttachments(origin, contexts);
      } else {
        await prepareAttachmentFiles(files, origin, false);
        if (!isAttachmentOrigin(origin)) return;
      }
    } catch (error) {
      reportAttachmentError(origin, error);
    } finally {
      finishAttachments(origin);
    }
  };

  const handlePaste = useCallback(async (e: React.ClipboardEvent<HTMLTextAreaElement> | ClipboardEvent) => {
    const data = e.clipboardData;
    if (!data) return;
    const files = Array.from(data.files || []);
    if (!files.length) {
      for (const item of Array.from(data.items || [])) {
        if (item.kind !== 'file') continue;
        const file = item.getAsFile();
        if (file) files.push(file);
      }
    }
    if (!files.length) return;
    e.preventDefault();
    const origin = beginAttachments();
    try {
      await prepareAttachmentFiles(files, origin, true);
      if (!isAttachmentOrigin(origin)) return;
    } catch (error) {
      reportAttachmentError(origin, error);
    } finally {
      finishAttachments(origin);
    }
  }, [beginAttachments, prepareAttachmentFiles, isAttachmentOrigin, reportAttachmentError, finishAttachments]);

  // Native Tauri drag & drop event listener
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const setupTauriDrop = async () => {
      try {
        if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
          const { getCurrentWebview } = await import('@tauri-apps/api/webview');
          if (disposed) return;
          unlisten = await getCurrentWebview().onDragDropEvent(async (event) => {
            if (disposed) return;
            if (event.payload.type === 'enter' || event.payload.type === 'over') {
              setIsDraggingOver(true);
            } else if (event.payload.type === 'leave') {
              setIsDraggingOver(false);
            } else if (event.payload.type === 'drop') {
              setIsDraggingOver(false);
              const paths = event.payload.paths;
              if (paths && paths.length > 0) {
                const origin = beginAttachments();
                try {
                  const contexts = await invoke<AttachedContext[]>('inspect_paths', { paths });
                  if (disposed || !isAttachmentOrigin(origin)) return;
                  appendAttachments(origin, contexts);
                } catch (error) {
                  if (!disposed) reportAttachmentError(origin, error);
                } finally {
                  finishAttachments(origin);
                }
              }
            }
          });
          if (disposed) unlisten();
        }
      } catch (err) {
        // Outside Tauri or test env
      }
    };

    setupTauriDrop();
    return () => {
      disposed = true;
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
    let disposed = false;
    const origin = { ...attachmentScopeRef.current };
    if (onListSkills && !isStreaming) {
      onListSkills()
        .then((skills) => { if (!disposed && isAttachmentOrigin(origin)) setAvailableSkills(skills); })
        .catch((error) => { if (!disposed && isAttachmentOrigin(origin)) setSkillsError(String(error)); });
    }
    return () => { disposed = true; };
  }, [onListSkills, isStreaming, currentSessionId, workspaceCwd, isAttachmentOrigin]);

  type ViewAnchor = { id: string; offset: number };
  const visibleAnchorRef = useRef<ViewAnchor | null>(null);
  const [activeMessageIndex, setActiveMessageIndex] = useState(-1);
  const activeMessageIndexRef = useRef(-1);
  const updateActiveMessageIndex = useCallback((index: number) => {
    // Streaming/layout commits may revisit the same anchor without a state write.
    if (activeMessageIndexRef.current === index) return;
    activeMessageIndexRef.current = index;
    setActiveMessageIndex(index);
  }, []);
  const pendingAnchorRef = useRef<ViewAnchor | null>(null);
  const firstMessageRef = useRef(messages[0]?.id);
  const viewportCacheRef = useRef(new Map<string | null | undefined, { anchor: ViewAnchor | null; bottom: boolean }>());
  const viewportSessionRef = useRef(currentSessionId);
  const wasLoadingRef = useRef(isSessionLoading);
  const navigationResetRef = useRef<number | null>(null);
  const correctionRef = useRef<ViewAnchor | null>(null);
  const correctionTimerRef = useRef<number | null>(null);
  const upwardIntentRef = useRef(false);
  const lastScrollTopRef = useRef(0);
  const touchYRef = useRef<number | null>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const jumpSettleRef = useRef<{ correct: () => void; cancel: () => void } | null>(null);
  const cancelAnchorCorrection = () => {
    correctionRef.current = null;
    if (correctionTimerRef.current !== null) {
      window.clearTimeout(correctionTimerRef.current);
      correctionTimerRef.current = null;
    }
    jumpSettleRef.current?.cancel();
  };
  const requestRef = useRef<object | null>(null);
  const captureAnchor = useCallback((allowFallback = true): ViewAnchor | null => {
    const element = parentRef.current;
    if (!element) return null;
    const top = element.getBoundingClientRect().top;
    for (const row of element.querySelectorAll<HTMLElement>('[data-index][data-message-id]')) {
      const rect = row.getBoundingClientRect();
      if (rect.bottom > top && rect.top < top + element.clientHeight) {
        return { id: row.dataset.messageId!, offset: rect.top - top };
      }
    }
    return allowFallback ? visibleAnchorRef.current : null;
  }, []);
  const loadHistory = async () => {
    if (!onLoadOlderMessages || loadingOlderMessages || isSessionLoading || requestRef.current) return;
    const request = {};
    requestRef.current = request;
    pendingAnchorRef.current = captureAnchor();
    visibleAnchorRef.current = pendingAnchorRef.current;
    stickToBottomRef.current = false;
    try { await onLoadOlderMessages(); }
    catch { /* The hook supplies the inline history error. */ }
    finally { if (requestRef.current === request) requestRef.current = null; }
  };

  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 140,
    overscan: 1,
    getItemKey: (index) => messages[index]?.id || index,
  });

  virtualizer.shouldAdjustScrollPositionOnItemSizeChange = (item) => {
    const anchor = correctionRef.current ?? pendingAnchorRef.current ?? visibleAnchorRef.current;
    const index = anchor ? messages.findIndex((message) => message.id === anchor.id) : -1;
    return !stickToBottomRef.current && item.index < index;
  };

  const handleJumpToBottom = useCallback(() => {
    cancelAnchorCorrection();
    upwardIntentRef.current = false;
    stickToBottomRef.current = true;
    setShowJumpBottom(false);
    if (parentRef.current) {
      parentRef.current.scrollTop = parentRef.current.scrollHeight;
    }
    if (messages.length > 0) {
      virtualizer.scrollToIndex(messages.length - 1, { align: 'end' });
    }
  }, [messages.length, virtualizer]);

  const handleJumpToMessage = useCallback((index: number, align: 'start' | 'center' = 'start') => {
    const messageId = messages[index]?.id;
    if (!messageId) return;
    cancelAnchorCorrection();
    upwardIntentRef.current = false;
    stickToBottomRef.current = false;
    // An explicit jump supersedes any older prepend/viewport anchor.
    pendingAnchorRef.current = null;
    visibleAnchorRef.current = null;
    updateActiveMessageIndex(index);
    virtualizer.scrollToIndex(index, { align });
    // Keep list-local virtual coordinates unchanged. A scrollMargin would also
    // require rebasing every translated row; mounted DOM includes the variable
    // history header and padding without changing prepend anchor coordinates.
    const element = parentRef.current;
    if (!element) return;
    let frame = 0;
    let remaining = 3;
    let timer: number | undefined;
    let target: HTMLElement | null = null;
    let stopped = false;
    const cancel = () => {
      if (stopped) return;
      stopped = true;
      observer.disconnect();
      cancelAnimationFrame(frame);
      if (timer !== undefined) window.clearTimeout(timer);
      for (const event of inputEvents) element.removeEventListener(event, cancel);
      if (jumpSettleRef.current === request) jumpSettleRef.current = null;
    };
    const correct = () => {
      if (stopped) return;
      // Numeric index is selector-safe even when message IDs contain CSS syntax.
      // Only inspect this viewport's mounted target, never the entire document.
      const row = element.querySelector<HTMLElement>(`[data-index="${index}"]`);
      if (!row || row.dataset.messageId !== messageId) return;
      if (row !== target) {
        if (target) observer.unobserve(target);
        target = row;
        observer.observe(row);
      }
      const rect = row.getBoundingClientRect();
      const view = element.getBoundingClientRect();
      if (!rect.height || !view.height) return;
      const delta = align === 'center'
        ? (rect.top + rect.bottom - view.top - view.bottom) / 2
        : rect.top - view.top;
      // Once mounted, correct actual pixels rather than reapplying an estimate.
      // Native scroll events notify TanStack of this offset change.
      if (Math.abs(delta) > 0.5) element.scrollTop += delta;
      lastScrollTopRef.current = element.scrollTop;
    };
    const observer = new window.ResizeObserver(correct);
    const request = { correct, cancel };
    const inputEvents = ['wheel', 'pointerdown', 'touchstart', 'keydown'] as const;
    jumpSettleRef.current = request;
    for (const event of inputEvents) element.addEventListener(event, cancel, { passive: true });
    observer.observe(element);
    const settle = () => {
      if (stopped) return;
      correct();
      if (--remaining > 0) frame = requestAnimationFrame(settle);
    };
    // Resize notifications and layout commits can correct late
    // measurements without an unbounded RAF loop or retaining observers forever.
    timer = window.setTimeout(cancel, 800);
    correct();
    frame = requestAnimationFrame(settle);
    lastScrollTopRef.current = element.scrollTop;
    const distanceToBottom = element.scrollHeight - element.scrollTop - element.clientHeight;
    setShowJumpBottom(distanceToBottom >= 35 && element.scrollHeight > element.clientHeight + 40);
    return cancel;
  }, [messages, virtualizer, updateActiveMessageIndex]);

  // Keep the actual visible row fixed while estimates settle after a prepend.
  const correctAnchor = useCallback(() => {
    const anchor = correctionRef.current;
    const element = parentRef.current;
    if (!anchor || !element) return;
    const row = Array.from(element.querySelectorAll<HTMLElement>('[data-index][data-message-id]'))
      .find((item) => item.dataset.messageId === anchor.id);
    if (row) element.scrollTop += row.getBoundingClientRect().top - element.getBoundingClientRect().top - anchor.offset;
    lastScrollTopRef.current = element.scrollTop;
  }, []);

  useLayoutEffect(() => {
    const changedSession = viewportSessionRef.current !== currentSessionId;
    const startedLoading = isSessionLoading && !wasLoadingRef.current;
    // Reset preview saat navigasi bukan permintaan eksplisit kembali ke pesan terbaru.
    if ((changedSession || isSessionLoading || wasLoadingRef.current) && jumpToMessage?.messageId === '') {
      navigationResetRef.current = jumpToMessage.request;
    }
    if (changedSession || startedLoading) {
      cancelAnchorCorrection();
      const cache = viewportCacheRef.current;
      cache.delete(viewportSessionRef.current);
      cache.set(viewportSessionRef.current, { anchor: visibleAnchorRef.current, bottom: stickToBottomRef.current });
      while (cache.size > 6) cache.delete(cache.keys().next().value);
      requestRef.current = null;
      pendingAnchorRef.current = null;
      correctionRef.current = null;
      upwardIntentRef.current = false;
    }
    const finishedLoading = wasLoadingRef.current && !isSessionLoading;
    wasLoadingRef.current = isSessionLoading;
    viewportSessionRef.current = currentSessionId;
    if (isSessionLoading) return;

    if (changedSession || finishedLoading) {
      const cached = viewportCacheRef.current.get(currentSessionId);
      const anchor = cached?.anchor;
      const index = anchor ? messages.findIndex((message) => message.id === anchor.id) : -1;
      stickToBottomRef.current = !cached || cached.bottom || index < 0;
      setShowJumpBottom(!stickToBottomRef.current);
      if (!stickToBottomRef.current && anchor) {
        pendingAnchorRef.current = anchor;
      }
    }
    const prepended = firstMessageRef.current !== messages[0]?.id;
    firstMessageRef.current = messages[0]?.id;
    const anchor = pendingAnchorRef.current;
    if (anchor && (prepended || changedSession || finishedLoading)) {
      const index = messages.findIndex((message) => message.id === anchor.id);
      if (index >= 0 && parentRef.current) {
        cancelAnchorCorrection();
        correctionRef.current = anchor;
        // Pemulihan navigasi dibatasi; prepend tetap menjaga resize terlambat hingga ada input.
        if (changedSession || finishedLoading) {
          correctionTimerRef.current = window.setTimeout(() => {
            correctionRef.current = null;
            correctionTimerRef.current = null;
          }, 750);
        }
        virtualizer.scrollToIndex(index, { align: 'start' });
        correctAnchor();
      }
      pendingAnchorRef.current = null;
    } else if (stickToBottomRef.current && messages.length) {
      virtualizer.scrollToIndex(messages.length - 1, { align: 'end' });
      visibleAnchorRef.current = { id: messages[messages.length - 1]?.id, offset: 0 };
      updateActiveMessageIndex(messages.length - 1);
    } else {
      visibleAnchorRef.current = captureAnchor();
      updateActiveMessageIndex(messages.findIndex((message) => message.id === visibleAnchorRef.current?.id));
    }
    lastScrollTopRef.current = parentRef.current?.scrollTop ?? 0;
  }, [messages, currentSessionId, isSessionLoading, jumpToMessage]);

  useLayoutEffect(() => {
    correctAnchor();
  });

  useEffect(() => {
    if (isSessionLoading) return;
    const element = listRef.current;
    if (!element) return;
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(correctAnchor);
    });
    observer.observe(element);
    for (const row of element.querySelectorAll<HTMLElement>('[data-index]')) observer.observe(row);
    return () => { observer.disconnect(); cancelAnimationFrame(frame); };
  }, [messages, isSessionLoading, correctAnchor]);

  const [outlinedMessageId, setOutlinedMessageId] = useState<string | null>(null);
  useEffect(() => {
    setOutlinedMessageId(jumpToMessage?.messageId || null);
    if (!jumpToMessage?.messageId) return;
    const timer = window.setTimeout(() => setOutlinedMessageId(null), 1800);
    return () => window.clearTimeout(timer);
  }, [jumpToMessage]);

  // Explicit search navigation runs after normal session/bottom restoration.
  useLayoutEffect(() => {
    if (!jumpToMessage || isSessionLoading) return;
    if (!jumpToMessage.messageId) {
      if (navigationResetRef.current !== jumpToMessage.request) handleJumpToBottom();
      return;
    }
    const index = messages.findIndex((message) => message.id === jumpToMessage.messageId);
    if (index < 0) return;
    return handleJumpToMessage(index, 'center');
  }, [jumpToMessage, isSessionLoading, currentSessionId]);

  const [isInChatSearchOpen, setIsInChatSearchOpen] = useState(false);
  const [inChatSearchQuery, setInChatSearchQuery] = useState('');
  const [currentMatchIdx, setCurrentMatchIdx] = useState(0);
  const inChatSearchInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
        const target = e.target as HTMLElement | null;
        if (target && typeof target.closest === 'function' && target.closest('[data-modal-container], [data-spotlight-bar]')) return;
        e.preventDefault();
        setIsInChatSearchOpen(true);
        window.setTimeout(() => inChatSearchInputRef.current?.focus(), 50);
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  const inChatMatches = useMemo(() => {
    const query = inChatSearchQuery.trim().toLowerCase();
    if (!query) return [];
    const matches: number[] = [];
    messages.forEach((msg, idx) => {
      const content = (msg.content || '').toLowerCase();
      const reasoning = (msg.reasoning || '').toLowerCase();
      let toolText = '';
      if (msg.toolCalls) {
        toolText = msg.toolCalls.map((tc) => `${tc.name} ${tc.result || ''}`).join(' ').toLowerCase();
      }
      if (content.includes(query) || reasoning.includes(query) || toolText.includes(query)) {
        matches.push(idx);
      }
    });
    return matches;
  }, [messages, inChatSearchQuery]);

  const handlePrevMatch = useCallback(() => {
    if (inChatMatches.length === 0) return;
    setCurrentMatchIdx((prev) => (prev <= 0 ? inChatMatches.length - 1 : prev - 1));
  }, [inChatMatches.length]);

  const handleNextMatch = useCallback(() => {
    if (inChatMatches.length === 0) return;
    setCurrentMatchIdx((prev) => (prev >= inChatMatches.length - 1 ? 0 : prev + 1));
  }, [inChatMatches.length]);

  useEffect(() => {
    if (!isInChatSearchOpen || inChatMatches.length === 0) return;
    const targetIndex = inChatMatches[currentMatchIdx];
    if (targetIndex != null) {
      handleJumpToMessage(targetIndex, 'center');
      if (messages[targetIndex]?.id) {
        setOutlinedMessageId(messages[targetIndex].id);
      }
    }
  }, [isInChatSearchOpen, currentMatchIdx, inChatMatches, handleJumpToMessage, messages]);

  const contextUsage = useMemo(() => {
    if (!messages || messages.length === 0) return null;
    const usedTokens = estimateSessionTokens(messages);
    const maxTokens = getModelContextWindow(
      effectiveConfig?.provider?.model,
      effectiveConfig?.provider?.contextWindow,
    );
    return { usedTokens, maxTokens };
  }, [messages, effectiveConfig?.provider?.model, effectiveConfig?.provider?.contextWindow]);

  useLayoutEffect(() => {
    jumpSettleRef.current?.correct();
    // Preserve an explicit jump's optimistic marker until its bounded settle ends.
    if (isSessionLoading || jumpSettleRef.current) return;
    const element = parentRef.current;
    if (element && stickToBottomRef.current && messages.length > 0) {
      const distanceToBottom = element.scrollHeight - element.scrollTop - element.clientHeight;
      if (distanceToBottom < 35 || element.scrollHeight <= element.clientHeight + 40) {
        updateActiveMessageIndex(messages.length - 1);
        return;
      }
    }
    // A scroll event can precede TanStack mounting the newly visible rows. Sample
    // only mounted, intersecting rows after every commit, never a cached fallback.
    const anchor = captureAnchor(false);
    if (!anchor) return;
    visibleAnchorRef.current = anchor;
    const index = messages.findIndex((message) => message.id === anchor.id);
    if (index >= 0) updateActiveMessageIndex(index);
  });

  useEffect(() => () => { requestRef.current = null; cancelAnchorCorrection(); }, []);

  const allSlashItems: SlashItem[] = useMemo(() => {
    const commands = [...COMMAND_ITEMS, ...TOOL_ITEMS];
    return [...commands, ...availableSkills.filter((skill) => !commands.some((item) => item.command.toLowerCase() === `/${skill.name.toLowerCase()}`))
      .map((skill): SlashItem => ({ id: `skill-${skill.name}`, category: 'SKILLS', command: `/${skill.name}`, description: skill.description, iconType: 'skill' }))];
  }, [availableSkills]);

  const slashMatch = useMemo(() => /(?:^|\s)\/([^\s/]*)$/.exec(input.slice(0, cursor)), [input, cursor]);
  const slashQuery = slashMatch ? slashMatch[1].toLowerCase() : null;

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

  const mentionMatch = useMemo(() => /(?:^|\s)@([^\s@]*)$/.exec(input.slice(0, cursor)), [input, cursor]);
  const mentionQuery = mentionMatch?.[1] || '';
  const mentionOpen = !menuDismissed && Boolean(mentionMatch);

  // Auto-scroll active item into view during keyboard navigation
  useEffect(() => {
    if (slashOpen) {
      const el = document.querySelector(`[data-slash-item="${activeIndex}"]`);
      el?.scrollIntoView({ block: 'nearest' });
    } else if (mentionOpen) {
      const el = document.querySelector(`[data-mention-item="${activeIndex}"]`);
      el?.scrollIntoView({ block: 'nearest' });
    }
  }, [activeIndex, slashOpen, mentionOpen]);

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
      if (!target.closest('[data-execution-menu]')) {
        setExecutionMenuOpen(false);
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
    const origin = beginAttachments();
    try {
      const selected = await open({ multiple: true });
      if (!isAttachmentOrigin(origin) || !selected) return;
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
      appendAttachments(origin, newAtts);
    } catch (error) {
      reportAttachmentError(origin, error);
    } finally {
      finishAttachments(origin);
    }
  };

  const handleAddFolder = async () => {
    setAddContextOpen(false);
    const origin = beginAttachments();
    try {
      const selected = await open({ directory: true, multiple: false });
      if (!isAttachmentOrigin(origin) || !selected || typeof selected !== 'string') return;
      const normalized = selected.replace(/\\/g, '/');
      const name = normalized.split('/').pop() || normalized;
      const newAtt: AttachedContext = {
        id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        path: selected,
        name,
        isDir: true,
      };
      appendAttachments(origin, [newAtt]);
    } catch (error) {
      reportAttachmentError(origin, error);
    } finally {
      finishAttachments(origin);
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
    setSkillsMenuOpen(true);
    setSkillsError(null);
    if (!onListSkills) { setSkillsError('Skill discovery is unavailable.'); return; }
    const origin = { ...attachmentScopeRef.current };
    setLoadingSkills(true);
    try {
      const skills = await onListSkills();
      if (isAttachmentOrigin(origin)) setAvailableSkills(skills);
    } catch (error) {
      if (isAttachmentOrigin(origin)) setSkillsError(String(error));
    } finally {
      if (isAttachmentOrigin(origin)) setLoadingSkills(false);
    }
  };

  const toggleSelectedSkill = (name: string) => {
    setSelectedSkills((previous) => previous.includes(name) ? previous.filter((skill) => skill !== name)
      : previous.length < 5 ? [...previous, name] : previous);
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
    const tokenStart = input.slice(0, cursor).lastIndexOf('/');
    if (tokenStart < 0) return;

    if (item.category === 'SKILLS') {
      const name = item.command.slice(1);
      if (!selectedSkills.includes(name) && selectedSkills.length >= 5) return;
      setSelectedSkills((previous) => previous.includes(name) ? previous : [...previous, name]);
      if (tokenStart === 0 && input.slice(tokenStart, cursor).trim() === input.trim()) {
        setInput('');
        setCursor(0);
      } else {
        const next = `${input.slice(0, tokenStart)}${item.command} ${input.slice(cursor)}`;
        const nextCursor = tokenStart + item.command.length + 1;
        setInput(next);
        setCursor(nextCursor);
        requestAnimationFrame(() => {
          textareaRef.current?.focus();
          textareaRef.current?.setSelectionRange(nextCursor, nextCursor);
        });
      }
      setMenuDismissed(true);
      textareaRef.current?.focus();
      return;
    }
    if (item.needsPrompt) {
      const nextVal = `${input.slice(0, tokenStart)}${item.command} ${input.slice(cursor)}`;
      const nextCursor = tokenStart + item.command.length + 1;
      setInput(nextVal);
      setCursor(nextCursor);
      setMenuDismissed(true);
      requestAnimationFrame(() => {
        textareaRef.current?.focus();
        textareaRef.current?.setSelectionRange(nextCursor, nextCursor);
      });
    } else {
      if (tokenStart === 0 && input.slice(0, cursor).trim() === input.trim()) {
        executeSlashCommand(item);
      } else {
        const nextVal = `${input.slice(0, tokenStart)}${item.command} ${input.slice(cursor)}`;
        const nextCursor = tokenStart + item.command.length + 1;
        setInput(nextVal);
        setCursor(nextCursor);
        setMenuDismissed(true);
      }
    }
  };

  const executeSlashCommand = (item: SlashItem, argText?: string, clearComposer = true) => {
    if (clearComposer) {
      invalidateAttachments();
      setInput('');
      setMenuDismissed(false);
      setCursor(0);
    }

    const arg = argText?.trim() || '';

    switch (item.command) {
      case '/clear':
        invalidateAttachments();
        setAttachedContexts([]);
        onClearSession();
        break;
      case '/new': {
        invalidateAttachments();
        setAttachedContexts([]);
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
      case '/refine':
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
              if (undone.imageInputs?.length || undone.selectedSkills?.length) {
                await onSendMessage(undone.content || '', undone.attachments, { imageInputs: undone.imageInputs, selectedSkills: undone.selectedSkills });
              } else {
                await onSendMessage(undone.content || '', undone.attachments);
              }
            }
          });
        }
        break;
      case '/undo':
        if (onUndoTurn) {
          const origin = { ...attachmentScopeRef.current };
          void onUndoTurn().then((undone) => {
            if (undone && isAttachmentOrigin(origin)) {
              invalidateAttachments();
              preparedImageInputsRef.current = new Map((undone.imageInputs ?? []).map((image) => [image.sourcePath, { ...image }]));
              setInput(undone.content || '');
              setSelectedSkills(undone.selectedSkills ?? []);
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

  const runOptions = useMemo<RunOptions>(() => {
    const options: RunOptions = { executionMode, ...(selectedSkills.length ? { selectedSkills } : {}) };
    if (councilMode && councilModelA && councilModelB && settings) {
      const providerA = settings.providers.find((provider) => provider.id === councilModelA.providerId);
      const providerB = settings.providers.find((provider) => provider.id === councilModelB.providerId);
      if (providerA && providerB) {
        options.council = true;
        options.candidateA = { provider: { ...providerA, model: councilModelA.model }, thinking: effectiveConfig?.thinking ?? false, reasoningEffort: effectiveConfig?.reasoningEffort ?? 'medium' };
        options.candidateB = { provider: { ...providerB, model: councilModelB.model }, thinking: effectiveConfig?.thinking ?? false, reasoningEffort: effectiveConfig?.reasoningEffort ?? 'medium' };
      }
    }
    return options;
  }, [executionMode, councilMode, councilModelA, councilModelB, settings, effectiveConfig, selectedSkills]);

  useEffect(() => {
    const next = queuedPrompts[0];
    if (!next || next.sessionId !== currentSessionId || isSessionLoading || isHistoryPreview || isStreaming || isReviewing || runStatus?.phase === 'saving_results' || stopRequested || (stoppedWithQueue && !next.sendNow) || dispatchingQueueRef.current.has(currentSessionId)) return;
    dispatchingQueueRef.current.set(next.sessionId, next.id);
    const dispatch = async () => {
      let accepted = false;
      try {
        const clean = next.prompt.trim();
        const localCommand = next.options.selectedSkills?.length ? undefined : allSlashItems.find((item) => item.category !== 'SKILLS' && item.command === clean.split(/\s+/)[0].toLowerCase() && item.command !== '/plan' && item.command !== '/goal' && item.command !== '/grill-me' && item.command !== '/search' && item.command !== '/web' && (!/^\/(review|reflect|refine)$/.test(item.command) || clean.toLowerCase() === item.command));
        if (localCommand) {
          executeSlashCommand(localCommand, clean.split(/\s+/).slice(1).join(' '), false);
          accepted = true;
        } else {
          accepted = await onSendMessage(next.prompt, next.attachments, next.options);
        }
      } catch {
        // Keep the item available for an explicit retry.
      }
      if (dispatchingQueueRef.current.get(next.sessionId) !== next.id) return;
      dispatchingQueueRef.current.delete(next.sessionId);
      if (accepted) updateSessionQueue(next.sessionId, (items) => items.filter((item) => item.id !== next.id));
      else {
        pauseSessionQueue(next.sessionId, true);
        updateSessionQueue(next.sessionId, (items) => items.map((item) => item.id === next.id ? { ...item, sendNow: false } : item));
      }
    };
    void dispatch();
    // Hasil penerimaan tetap memperbarui antrean asal meskipun pengguna sudah pindah chat.
  }, [queuedPrompts, currentSessionId, isSessionLoading, isHistoryPreview, isStreaming, isReviewing, runStatus?.phase, stopRequested, stoppedWithQueue, onSendMessage, updateSessionQueue, pauseSessionQueue]); // eslint-disable-line react-hooks/exhaustive-deps

  const send = async () => {
    if (isSessionLoading || attachmentScopeRef.current.pending > 0) return;
    const origin = { ...attachmentScopeRef.current };
    let draft = input;
    const originalDraft = input;
    const draftSkills = [...selectedSkills];
    while (draft.startsWith('/')) {
      const token = draft.match(/^\/(\S+)(?:\s+|$)/);
      const name = availableSkills.find((skill) => skill.name.toLowerCase() === token?.[1].toLowerCase())?.name;
      if (!token || !name || [...COMMAND_ITEMS, ...TOOL_ITEMS].some((item) => item.command.toLowerCase() === `/${name.toLowerCase()}`)) break;
      if (!draftSkills.includes(name)) draftSkills.push(name);
      draft = draft.slice(token[0].length);
    }
    const inlineSkills = draft.matchAll(/(?:^|\s)\/([a-zA-Z0-9_-]+)(?=\s|$)/g);
    for (const match of inlineSkills) {
      const tokenName = match[1].toLowerCase();
      const matchedSkill = availableSkills.find((s) => s.name.toLowerCase() === tokenName)?.name;
      if (matchedSkill && !draftSkills.includes(matchedSkill) && draftSkills.length < 5) {
        draftSkills.push(matchedSkill);
      }
    }
    if (draftSkills.length > 5) { setAttachmentError('Select at most five skills for the next message.'); return; }
    const draftAttachments = attachedContexts;
    const reusedImages = draftAttachments.flatMap((attachment) => {
      const image = !attachment.isDir && preparedImageInputsRef.current.get(attachment.path);
      return image ? [{ ...image }] : [];
    });
    const draftOptions = { ...runOptions, ...(draftSkills.length ? { selectedSkills: draftSkills } : {}), ...(reusedImages.length ? { imageInputs: reusedImages } : {}) };
    const clean = (draft || '').trim();
    const slashItem = !draftSkills.length && clean.startsWith('/')
      ? allSlashItems.find((item) => item.category !== 'SKILLS' && item.command.toLowerCase() === clean.split(/\s+/)[0].toLowerCase() && (!/^\/(review|reflect|refine)$/.test(item.command) || clean.toLowerCase() === item.command))
      : undefined;
    // /stop must stay reachable while a turn streams so the run can always be interrupted.
    const interruptsRun = originalDraft.trim().toLowerCase() === '/stop';
    if (interruptsRun) { onStop(); setInput(''); return; }
    if ((!clean && draftAttachments.length === 0) || isReviewing) return;

    if (!isStreaming) {
      cancelAnchorCorrection();
      pendingAnchorRef.current = null;
      upwardIntentRef.current = false;
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
          options: draftOptions,
          sessionId: currentSessionId,
        },
      ]);
      setStoppedWithQueue(false);
      invalidateAttachments();
      setInput('');
      setAttachedContexts([]);
      setSelectedSkills([]);
      setCursor(0);
      return;
    }

    const clearIfUnchanged = (accepted: boolean) => {
      if (!accepted || !isAttachmentOrigin(origin)) return;
      invalidateAttachments();
      setInput((current) => (current === originalDraft ? '' : current));
      setAttachedContexts((current) => (current === draftAttachments ? [] : current));
      setSelectedSkills((current) => (current === selectedSkills ? [] : current));
      setCursor(0);
    };

    // /plan, /goal, /grill-me, /search, and /web are transformed by useAgent; they are runs, not local commands.
    if (slashItem && slashItem.command !== '/plan' && slashItem.command !== '/goal' && slashItem.command !== '/grill-me' && slashItem.command !== '/search' && slashItem.command !== '/web') {
      const args = clean.split(/\s+/).slice(1).join(' ').trim();
      executeSlashCommand(slashItem, args);
      setAttachedContexts([]);
      return;
    }

    const accepted = await onSendMessage(
      draft,
      draftAttachments.length > 0 ? draftAttachments : undefined,
      draftOptions,
    );
    clearIfUnchanged(accepted);
  };

  const selectMention = (path: string) => {
    if (!mentionMatch) return;
    const tokenStart = input.slice(0, cursor).lastIndexOf('@');
    if (tokenStart < 0) return;
    const before = input.slice(0, tokenStart);
    const after = input.slice(cursor);
    const next = `${before}${after.startsWith(' ') ? after.slice(1) : after}`;
    const nextCursor = tokenStart;
    setInput(next);
    setCursor(nextCursor);
    setMenuDismissed(true);

    const origin = beginAttachments();
    try {
      const isAbsolute = /^(?:[a-zA-Z]:[\\/]|\\\\|\/)/.test(path);
      const normalized = path.replace(/\\/g, '/');
      const name = normalized.split('/').filter(Boolean).pop() || normalized;
      const isDir = path.endsWith('/') || path.endsWith('\\');
      const fullPath = isAbsolute || !workspaceCwd
        ? path
        : `${workspaceCwd.replace(/[\\/]+$/, '')}/${normalized.replace(/^[\\/]+/, '')}`;
      const newAtt: AttachedContext = {
        id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        path: fullPath,
        name,
        isDir,
      };
      appendAttachments(origin, [newAtt]);
    } finally {
      finishAttachments(origin);
    }

    requestAnimationFrame(() => {
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(nextCursor, nextCursor);
    });
  };

  const handleSelectCouncilWinner = useCallback((messageId: string, winner: 'A' | 'B') => {
    onSelectCouncilWinner?.(messageId, winner);
  }, [onSelectCouncilWinner]);

  const handleAnswerQuestion = useCallback((answer: string) => {
    void onSendMessage(answer);
  }, [onSendMessage]);

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const itemCount = slashOpen ? slashMatches.length : mentionOpen ? fileOptions.length : 0;
    if (itemCount > 0 && ['ArrowDown', 'ArrowUp'].includes(event.key)) {
      event.preventDefault();
      const direction = event.key === 'ArrowUp' ? -1 : 1;
      setActiveIndex((index) => (index + direction + itemCount) % itemCount);
      return;
    }
    if (event.key === 'Tab') {
      if (slashOpen && slashMatches[activeIndex]) {
        event.preventDefault();
        handleSelectSlashItem(slashMatches[activeIndex]);
        return;
      }
      if (mentionOpen && fileOptions[activeIndex]) {
        event.preventDefault();
        selectMention(fileOptions[activeIndex]);
        return;
      }
    }
    if (event.key === 'Escape' && (slashOpen || mentionOpen)) {
      event.preventDefault();
      setMenuDismissed(true);
      return;
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      if (event.nativeEvent.isComposing) return;
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
        <div className="absolute inset-0 z-50 flex flex-col items-center justify-center bg-[#0d1117]/90 backdrop-blur-sm border-2 border-dashed border-[#58a6ff] m-2 rounded-xl pointer-events-none ui-enter select-none">
          <div className="flex h-14 w-14 items-center justify-center rounded-xl bg-[#161b22] border border-[#30363d] shadow-lg mb-3">
            <FileCode2 size={28} className="text-[#58a6ff]" />
          </div>
          <p className="text-sm font-semibold text-[#f0f6fc]">Drop files or folders here</p>
          <p className="text-xs text-[#8b949e] mt-1">Automatically attach as context to your prompt</p>
        </div>
      )}
      <ChatHeader
        onToggleSidebar={onToggleSidebar}
        sidebarOpen={sidebarOpen}
        sessionTitle={sessionTitle}
        totalMessageCount={totalMessageCount}
        onOpenLiveShell={onOpenLiveShell}
        councilMode={councilMode}
        setCouncilMode={setCouncilMode}
        councilSecondCandidate={councilSecondCandidate}
        onOpenModelPicker={onOpenModelPicker}
        effectiveConfig={effectiveConfig}
        subagents={subagents}
        onToggleArtifacts={onToggleArtifacts}
        artifactCount={artifactCount}
        isArtifactOpen={isArtifactOpen}
        onOpenSpotlight={onOpenSpotlight}
        onOpenInChatSearch={() => {
          setIsInChatSearchOpen(true);
          window.setTimeout(() => inChatSearchInputRef.current?.focus(), 50);
        }}
        onExportSession={onExportSession}
        onOpenSettings={onOpenSettings}
        onClearConversation={() => {
          invalidateAttachments();
          setAttachedContexts([]);
          onClearSession();
        }}
      />

      <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
        {isInChatSearchOpen && (
          <div
            className="absolute right-6 top-3 z-30 flex items-center gap-1.5 rounded-xl border border-[#30363d] bg-[#161b22]/95 px-2.5 py-1.5 shadow-2xl text-xs backdrop-blur-md"
            data-testid="in-chat-search-bar"
          >
            <Search size={13} className="text-[#8b949e] shrink-0" />
            <input
              ref={inChatSearchInputRef}
              type="text"
              value={inChatSearchQuery}
              onChange={(e) => {
                setInChatSearchQuery(e.target.value);
                setCurrentMatchIdx(0);
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  if (e.shiftKey) handlePrevMatch();
                  else handleNextMatch();
                } else if (e.key === 'Escape') {
                  e.preventDefault();
                  setIsInChatSearchOpen(false);
                }
              }}
              placeholder="Find in chat... (Ctrl+F)"
              className="w-36 sm:w-48 bg-transparent text-[#e6edf3] placeholder-[#6e7681] text-xs focus:outline-none"
              autoFocus
            />
            <span className="text-[11px] font-mono text-[#8b949e] px-1 select-none">
              {inChatSearchQuery.trim()
                ? inChatMatches.length > 0
                  ? `${currentMatchIdx + 1}/${inChatMatches.length}`
                  : '0/0'
                : ''}
            </span>
            <button
              type="button"
              onClick={handlePrevMatch}
              disabled={inChatMatches.length === 0}
              className="rounded p-1 text-[#8b949e] hover:bg-[#21262d] hover:text-[#e6edf3] disabled:opacity-40"
              title="Previous match (Shift+Enter)"
              aria-label="Previous match"
            >
              <ChevronUp size={13} />
            </button>
            <button
              type="button"
              onClick={handleNextMatch}
              disabled={inChatMatches.length === 0}
              className="rounded p-1 text-[#8b949e] hover:bg-[#21262d] hover:text-[#e6edf3] disabled:opacity-40"
              title="Next match (Enter)"
              aria-label="Next match"
            >
              <ChevronDown size={13} />
            </button>
            <button
              type="button"
              onClick={() => setIsInChatSearchOpen(false)}
              className="rounded p-1 text-[#8b949e] hover:bg-[#21262d] hover:text-[#e6edf3]"
              title="Close search (Esc)"
              aria-label="Close search"
            >
              <X size={13} />
            </button>
          </div>
        )}
        {isSessionLoading && <div role="status" className="absolute inset-0 z-10 flex items-center justify-center bg-[#0d1117] text-sm text-[#8b949e]">Loading conversation{'\u2026'}</div>}
        <div
          ref={parentRef}
          aria-hidden={isSessionLoading || undefined}
          inert={isSessionLoading || undefined}
          style={isSessionLoading ? { visibility: 'hidden' } : undefined}
          tabIndex={0}
          className="min-h-0 flex-1 overflow-y-auto px-4 py-5"
        onKeyDown={(event) => {
          if (['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' ', 'Spacebar'].includes(event.key)) {
            cancelAnchorCorrection();
          }
          if (['ArrowUp', 'PageUp', 'Home'].includes(event.key)) {
            cancelAnchorCorrection(); upwardIntentRef.current = true; stickToBottomRef.current = false;
          } else { upwardIntentRef.current = false; }
        }}
        onPointerDown={() => { cancelAnchorCorrection(); upwardIntentRef.current = true; }}
        onTouchStart={(event) => { touchYRef.current = event.touches[0]?.clientY ?? null; }}
        onWheel={(e) => {
          cancelAnchorCorrection();
          upwardIntentRef.current = e.deltaY < 0;
          if (e.deltaY < 0) {
            // User intentionally scrolled up with the mouse wheel
            stickToBottomRef.current = false;
          }
        }}
        onTouchMove={(event) => {
          cancelAnchorCorrection();
          const y = event.touches[0]?.clientY;
          upwardIntentRef.current = y !== undefined && touchYRef.current !== null && y > touchYRef.current;
          touchYRef.current = y ?? null;
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
          const upward = element.scrollTop < lastScrollTopRef.current;
          lastScrollTopRef.current = element.scrollTop;
          if (upwardIntentRef.current && upward && element.scrollTop <= 120) {
            upwardIntentRef.current = false;
            if (hasOlderMessages && !historyError) void loadHistory();
          }
          const hasOverflow = element.scrollHeight > element.clientHeight + 40;
          const distanceToBottom = element.scrollHeight - element.scrollTop - element.clientHeight;
          const isAtBottom = !hasOverflow || distanceToBottom < 35;
          if (isAtBottom) {
            cancelAnchorCorrection();
            stickToBottomRef.current = true;
            setShowJumpBottom(false);
            visibleAnchorRef.current = { id: messages[messages.length - 1]?.id, offset: 0 };
            updateActiveMessageIndex(messages.length - 1);
          } else {
            stickToBottomRef.current = false;
            setShowJumpBottom(hasOverflow);
            visibleAnchorRef.current = captureAnchor();
            updateActiveMessageIndex(messages.findIndex((message) => message.id === visibleAnchorRef.current?.id));
          }
        }}
      >
        <div className="mx-auto mb-4 flex max-w-3xl flex-wrap items-center justify-center gap-3 text-xs text-[#8b949e]">
          <span>{messages.length} / {totalMessageCount} messages loaded</span>
          {hasOlderMessages && onLoadOlderMessages && (
            <button type="button" disabled={loadingOlderMessages || isSessionLoading} onClick={() => void loadHistory()} className="flex items-center gap-2 rounded border border-[#30363d] px-3 py-2 disabled:opacity-50">
              {loadingOlderMessages && <LoaderCircle size={13} aria-hidden="true" className="animate-spin motion-reduce:animate-none" />}
              {loadingOlderMessages ? 'Loading older messages\u2026' : 'Load older messages'}
            </button>
          )}
          {historyError && <div role="alert" className="text-[#f85149]">{historyError}{' '}
            {onLoadOlderMessages && <button type="button" disabled={loadingOlderMessages || isSessionLoading} onClick={() => void loadHistory()} aria-label="Retry loading older messages" className="underline">Retry</button>}
          </div>}
        </div>
        {messages.length === 0 ? (
          <div className="ui-enter mx-auto flex h-full max-w-lg flex-col justify-center py-8 text-left">
            <h2 className="text-base font-medium text-[#e6edf3]">What would you like to work on?</h2>
            <p className="mt-2 text-sm leading-6 text-[#8b949e]">Describe a task, ask a question, or attach files for context.</p>
            <ul className="mt-4 space-y-1 text-xs leading-5 text-[#8b949e]">
              <li>Find the code responsible for a failing test.</li>
              <li>Explain how this workspace is organized.</li>
            </ul>
            <p className="mt-4 font-mono text-[11px] text-[#8b949e]">@ for workspace files &middot; / for commands</p>
          </div>
        ) : (
          <div ref={listRef} className="relative mx-auto w-full max-w-3xl" style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const message = messages[virtualRow.index];
              if (!message) return null;
              return (
                <div
                  key={virtualRow.key}
                  data-index={virtualRow.index}
                  data-message-id={message.id}
                  ref={virtualizer.measureElement}
                  className={`absolute left-0 top-0 w-full ${outlinedMessageId === message.id ? 'outline outline-2 outline-[#58a6ff] outline-offset-[-2px]' : ''}`}
                  style={{ transform: `translateY(${virtualRow.start}px)` }}
                >
                  <MessageRow
                    message={message}
                    onSelectArtifact={onSelectArtifact}
                    onSelectCouncilWinner={handleSelectCouncilWinner}
                    onRetryMessage={onRetryTurn}
                    onEditMessage={onEditAndResendMessage}
                    onRollbackMessage={onRollbackToMessage}
                    onAnswerQuestion={handleAnswerQuestion}
                    isStreaming={isStreaming}
                  />
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>

      {/* Conversation Timeline Scrubber */}
      {!isSessionLoading && <div role="region" aria-label={`Loaded message timeline: ${messages.length} of ${totalMessageCount} messages loaded`}>
      <TimelineScrubber
        messages={messages}
        sessionTitle={sessionTitle}
        scrollElement={parentRef.current}
        activeMessageIndex={activeMessageIndex}
        onJumpToMessage={handleJumpToMessage}
      />
      </div>}

      {/* Floating Jump to Bottom Button */}
      {showJumpBottom && !isSessionLoading && (
        <button
          type="button"
          onClick={() => {
            if (isHistoryPreview) {
              void onLoadLatestMessages?.();
            } else {
              handleJumpToBottom();
            }
          }}
          aria-label={isStreaming ? 'Jump to bottom' : 'Return to latest'}
          className="absolute bottom-20 right-8 z-30 flex items-center gap-1.5 rounded-lg border border-[#30363d] bg-[#161b22]/95 px-3 py-1.5 text-xs text-[#e6edf3] shadow-lg backdrop-blur-md hover:border-[#58a6ff]/60 hover:bg-[#21262d] transition-colors ui-enter"
        >
          <ChevronDown size={13} className="text-[#58a6ff]" />
          <span className="font-medium text-[11px]">
            {isStreaming ? 'New output' : 'Return to latest'}
          </span>
          {isStreaming && (
            <span className="relative flex h-2 w-2">
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
                <span className="font-mono text-[10px] font-bold text-[#8b949e]">A</span>
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
                <span className="font-mono text-[10px] font-bold text-[#8b949e]">B</span>
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

        <ActivityTray
          key={currentSessionId ?? 'unsaved-session'}
          subagents={subagents}
          tasks={backgroundTasks}
          activeSkills={isStreaming ? activeSkills : undefined}
          onSelectArtifact={onSelectArtifact}
          onKillTask={onKillBackgroundTask}
          onRefreshTasks={onRefreshBackgroundTasks}
        />

        {slashOpen && (
          <div className="ui-menu absolute bottom-[calc(100%-4px)] left-3 right-3 z-40 mx-auto max-w-3xl overflow-hidden rounded-xl border border-[#30363d] bg-[#161b22]/98 shadow-lg backdrop-blur-md">
            {/* Active item usage tooltip bubble */}
            {slashMatches[activeIndex] && (slashMatches[activeIndex].usage || slashMatches[activeIndex].description) && (
              <div className="hidden lg:flex absolute right-3 top-2.5 z-50 max-w-xs flex-col rounded-lg border border-[#30363d] bg-[#0d1117]/95 p-2 text-xs shadow-lg backdrop-blur-md">
                <span className="font-mono text-[11px] font-bold text-[#8b949e]">
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
                            ? 'border border-[#30363d] bg-[#21262d] text-[#e6edf3]'
                            : 'text-[#c9d1d9] hover:bg-[#21262d]'
                        }`}
                      >
                        <div className="flex min-w-0 items-center gap-2">
                          {item.iconType === 'terminal' ? (
                            <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded border border-[#30363d] bg-[#0d1117] font-mono text-[9px] font-bold text-[#8b949e]">
                              &gt;_
                            </span>
                          ) : (
                            <span className="flex h-4 w-4 shrink-0 items-center justify-center text-[#8b949e]">
                              <BookOpen size={13} className="text-[#8b949e]" />
                            </span>
                          )}
                          <span className={`truncate font-mono font-semibold ${isActive ? 'text-[#e6edf3]' : 'text-[#f0f6fc]'}`}>
                            {item.command}
                          </span>
                        </div>
                        <div className="hidden shrink-0 items-center text-right text-xs text-[#8b949e] sm:flex">
                          {item.iconType === 'skill' && (
                            <span className="mr-1 font-mono text-[9px] text-[#8b949e]/80">skill</span>
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
          <div className="ui-menu absolute bottom-[calc(100%-4px)] left-3 right-3 z-20 mx-auto max-h-64 max-w-3xl overflow-y-auto rounded-lg border border-[#30363d] bg-[#161b22] p-1.5 shadow-lg">
            <div className="flex items-center gap-1.5 px-2 py-1 text-[10px] uppercase tracking-wider text-[#8b949e]"><AtSign size={11} /> Workspace files</div>
            {loadingFiles && <div className="px-3 py-3 text-xs text-[#8b949e]">Searching workspace…</div>}
            {!loadingFiles && fileOptions.length === 0 && <div className="px-3 py-3 text-xs text-[#8b949e]">No matching files</div>}
            {fileOptions.map((path, index) => (
              <button key={path} type="button" data-mention-item={index} onClick={() => selectMention(path)} onMouseEnter={() => setActiveIndex(index)} className={`flex w-full items-center gap-2 rounded px-3 py-2 text-left font-mono text-[11px] ${index === activeIndex ? 'bg-[#21262d] text-[#e6edf3]' : 'text-[#8b949e]'}`}>
                <File size={12} className="shrink-0" /><span className="truncate">{path}</span>
              </button>
            ))}
          </div>
        )}
        {/* Queued Prompts Banner */}
        {queuedPrompts.length > 0 && (
          <div
            className="mx-auto mb-2.5 w-full max-w-3xl space-y-1.5 ui-enter"
            data-testid="queued-prompts-banner"
          >
            <div className="flex items-center justify-between px-1 text-[11px] font-medium text-[#8b949e]">
              <span className="flex items-center gap-1.5 font-mono text-[10px] text-[#8b949e]">
                <Clock size={11} />
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
                className="flex items-center justify-between gap-2 rounded-xl border border-[#30363d] bg-[#161b22]/95 px-3 py-1.5 text-xs text-[#e6edf3] shadow-md backdrop-blur-sm transition-colors hover:border-[#58a6ff]/40"
                data-testid={`queued-prompt-${index}`}
              >
                <div className="flex items-center gap-2 min-w-0 flex-1">
                  <span className="shrink-0 rounded bg-[#30363d]/70 px-1.5 py-0.5 font-mono text-[10px] text-[#8b949e]">
                    #{index + 1}
                  </span>
                  <p className="truncate text-xs text-[#e6edf3]" title={item.prompt}>
                    {item.prompt}
                  </p>
                  {item.attachments && item.attachments.length > 0 && (
                    <span className="shrink-0 flex items-center gap-1 rounded bg-[#21262d] px-1.5 py-0.5 text-[10px] font-mono text-[#8b949e]">
                      <Paperclip size={10} /> +{item.attachments.length}
                    </span>
                  )}
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  <button
                    type="button"
                    onClick={() => handleSendQueuedNow(item.id)}
                    disabled={queueActionsBlocked || queuedPrompts.some((queued) => queued.sendNow)}
                    className="rounded p-1.5 text-[#8b949e] hover:bg-[#21262d] hover:text-[#58a6ff] disabled:opacity-40 transition-colors"
                    aria-label="Send queued prompt now"
                    title="Send now (stops the current run)"
                  >
                    {item.sendNow ? <LoaderCircle size={13} className="animate-spin" /> : <ArrowRight size={13} />}
                  </button>
                  <button
                    type="button"
                    onClick={() => handleEditQueued(item.id)}
                    disabled={dispatchingQueueRef.current.has(currentSessionId)}
                    className="rounded p-1.5 text-[#8b949e] hover:bg-[#21262d] hover:text-[#58a6ff] disabled:opacity-40 transition-colors"
                    aria-label="Edit queued prompt"
                    title="Edit prompt in composer"
                  >
                    <Pencil size={13} />
                  </button>
                  <button
                    type="button"
                    onClick={() => handleRemoveQueued(item.id)}
                    disabled={dispatchingQueueRef.current.has(currentSessionId)}
                    className="rounded p-1.5 text-[#8b949e] hover:bg-[#21262d] hover:text-[#f85149] disabled:opacity-40 transition-colors"
                    aria-label="Remove queued prompt"
                    title="Remove from queue"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        <RunStatusBar
          status={isHistoryPreview ? null : runStatus}
          onCancelPreparation={onStop}
          contextUsage={contextUsage}
          onCompact={onCompactSession}
        />
        <div className="mx-auto max-w-3xl rounded-xl border border-[#30363d] bg-[#161b22] transition-colors focus-within:border-[#58a6ff]">
          {selectedSkills.length > 0 && (
            <div aria-label="Selected skills" className="flex max-h-20 flex-wrap gap-1.5 overflow-y-auto border-b border-[#30363d]/50 px-3 py-2">
              {selectedSkills.map((name) => <span key={name} className="inline-flex min-w-0 items-center gap-1.5 rounded-md border border-[#30363d] bg-[#0d1117] px-2 py-1 text-[11px] text-[#a5d6ff]">
                <BookOpen size={12} className="shrink-0" /><span className="max-w-48 truncate">{name}</span>
                <button type="button" aria-label={`Remove skill ${name}`} onClick={() => setSelectedSkills((previous) => previous.filter((skill) => skill !== name))} className="rounded text-[#8b949e] hover:text-[#e6edf3]"><X size={12} /></button>
              </span>)}
            </div>
          )}
          {pendingAttachments > 0 && <div role="status" className="px-3 py-2 text-xs text-[#8b949e]">Preparing attachments ({pendingAttachments})...</div>}
          {attachmentError && <div role="alert" className="px-3 py-2 text-xs text-[#f85149]">{attachmentError}</div>}
          {attachedContexts.some((att) => !att.isDir && /\.(svg|bmp|tiff?|heic|heif|avif)$/i.test(att.name)) && (
            <div
              role="note"
              title="Unsupported image format: attached as file context, not sent as pixels for vision."
              className="px-3 py-2 text-xs leading-5 text-[#d29922]"
            >
              Unsupported image formats stay attached as file context; their pixels are not sent for vision.
              {' '}Vision attachments support PNG, JPEG, GIF, and WebP only. Convert to a supported format for visual analysis.
              {' '}SVG files can still be read as text by tools.
            </div>
          )}
          {attachedContexts.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5 border-b border-[#30363d]/50 px-3 py-2">
              {attachedContexts.map((att) => (
                <span
                  key={att.id}
                  className="inline-flex items-center gap-1.5 rounded-lg border border-[#30363d] bg-[#0d1117] px-2 py-0.5 text-[11px] font-mono text-[#e6edf3]"
                >
                  {att.isDir ? (
                    <Folder size={11} className="text-[#8b949e]" />
                  ) : /\.(png|jpe?g|gif|webp|svg|bmp)$/i.test(att.name) ? (
                    <ImageIcon size={11} className="text-[#8b949e]" />
                  ) : (
                    <Paperclip size={11} className="text-[#8b949e]" />
                  )}
                  <AttachmentPreview
                    attachment={att}
                    sessionId={currentSessionId}
                    workspaceCwd={workspaceCwd}
                    allowPreview={previewAttachmentIds.has(att.id)}
                    model={councilMode && councilModelA && councilModelB
                      ? [councilModelA.model, councilModelB.model].find((model) => /^(deepseek-chat|deepseek-reasoner)$/i.test(model.trim())) ?? councilModelA.model
                      : effectiveConfig?.provider.model}
                  />
                  <span className="max-w-[200px] truncate" title={att.path}>{att.name}</span>
                  <button
                    type="button"
                    onClick={() => { invalidateAttachments(); setAttachedContexts((prev) => prev.filter((item) => item.id !== att.id)); }}
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
              <div className="flex shrink-0 select-none items-center gap-1 pl-3.5 pt-3.5 font-mono text-xs font-bold text-[#8b949e]" title="Command mode">
                &gt;_
              </div>
            )}
            <textarea
              disabled={isSessionLoading || isHistoryPreview}
              ref={textareaRef}
              value={input}
              rows={2}
              onChange={(event) => {
                setInput(event.target.value);
                const pos = event.target.selectionStart;
                setCursor(pos !== null && pos !== undefined && pos > 0 ? pos : event.target.value.length);
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
          <div className="flex flex-wrap items-center justify-between gap-2 px-3 pb-2.5 pt-0.5">
            {/* Left cluster: [+] and [Model / Thinking pill] side-by-side */}
            <div className="flex min-w-0 flex-wrap items-center gap-1.5">
              <div className="relative" data-context-menu>
                <button
                  type="button"
                  onClick={() => {
                    setAddContextOpen((value) => !value);
                    setSkillsMenuOpen(false);
                  }}
                  className="flex h-8 w-8 items-center justify-center rounded-lg border border-[#30363d] bg-[#21262d] text-[#8b949e] transition-colors hover:bg-[#30363d] hover:text-[#e6edf3]"
                  title="Add Context (Files, Folder, Mentions)"
                  aria-label="Add Context"
                >
                  <Plus size={14} />
                </button>
                {addContextOpen && (
                  <div className="ui-menu absolute bottom-9 left-0 z-30 w-44 rounded-xl border border-[#30363d] bg-[#161b22] p-1 shadow-lg">
                    <div className="px-2.5 py-1.5 text-[10px] font-semibold uppercase tracking-wider text-[#8b949e]">Add Context</div>
                    <button
                      type="button"
                      onClick={handleAddFiles}
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <Paperclip size={14} className="text-[#8b949e]" /> Files
                    </button>
                    <button
                      type="button"
                      onClick={handleAddFolder}
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <Folder size={14} className="text-[#8b949e]" /> Folder
                    </button>
                    <button
                      type="button"
                      onClick={handleAddMention}
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <AtSign size={14} className="text-[#8b949e]" /> Mentions
                    </button>
                    <button
                      type="button"
                      onClick={handleOpenSkillsList}
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <BookOpen size={14} className="text-[#8b949e]" /> Actions & Skills
                    </button>
                    <button
                      type="button"
                      onClick={handleAddBrowser}
                      className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-xs text-[#e6edf3] hover:bg-[#21262d]"
                    >
                      <Globe size={14} className="text-[#8b949e]" /> Browser
                    </button>
                  </div>
                )}
                {skillsMenuOpen && <SkillPicker skills={availableSkills} selected={selectedSkills} loading={loadingSkills} error={skillsError}
                  onToggle={toggleSelectedSkill} onClose={() => { setSkillsMenuOpen(false); textareaRef.current?.focus(); }} />}
              </div>

              {/* Model & Thinking pill */}
              <div className="relative" data-thinking-menu>
                <button
                  type="button"
                  onClick={() => {
                    setThinkingMenuOpen((value) => !value);
                    setAddContextOpen(false);
                    setSkillsMenuOpen(false);
                  }}
                  className="flex h-8 items-center gap-1.5 rounded-lg border border-[#30363d] bg-[#21262d] px-2.5 text-xs text-[#e6edf3] transition-colors hover:bg-[#30363d]"
                  title="Model & Thinking controls"
                >
                  <SlidersHorizontal size={12} className="shrink-0 text-[#58a6ff]" />
                  <span className="max-w-[100px] truncate font-mono text-[11px] font-medium sm:max-w-[180px]">
                    {effectiveConfig?.provider.model || 'Select model'}
                  </span>
                  <span
                    className={`rounded px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-wider ${
                      effectiveConfig?.thinking
                        ? 'bg-[#30363d] text-[#e6edf3]'
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
                <div className="ui-menu fixed bottom-24 left-3 right-3 z-50 w-auto rounded-xl border border-[#30363d] bg-[#161b22] p-2.5 shadow-lg sm:absolute sm:bottom-9 sm:left-0 sm:right-auto sm:w-72">
                  <div className="mb-2.5 flex items-center justify-between border-b border-[#30363d] pb-2">
                    <div className="flex items-center gap-1.5">
                      <Brain size={14} className="text-[#8b949e]" />
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
                      <span className="font-mono text-[10px] text-[#8b949e] capitalize">
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

            {/* Right cluster: Execution Mode, Voice Dictation, and Send / Stop button */}
            <div className="flex items-center gap-2">
              {isReviewing && <span role="status" className="flex items-center gap-1.5 text-[11px] text-[#8b949e]"><Brain size={12} /> Reviewing…</span>}

              {/* Execution Mode Pill & Dropdown */}
              <div className="relative" data-execution-menu>
                <button
                  type="button"
                  onClick={() => {
                    setExecutionMenuOpen((value) => !value);
                    setThinkingMenuOpen(false);
                    setAddContextOpen(false);
                    setSkillsMenuOpen(false);
                  }}
                  className="flex h-8 items-center gap-1.5 rounded-lg border border-[#30363d] bg-[#21262d] px-2 text-xs text-[#e6edf3] transition-colors hover:bg-[#30363d]"
                  title={`Execution mode: ${activeExecutionMode.title}`}
                  aria-label="Execution mode"
                >
                  <ActiveModeIcon size={13} className="shrink-0 text-[#8b949e]" />
                  <span className="font-medium text-[11px]">
                    {activeExecutionMode.title.replace(' (Preview)', '')}
                  </span>
                  <ChevronUp size={12} className={`text-[#8b949e] transition-transform ${executionMenuOpen ? 'rotate-180' : ''}`} />
                </button>

                {executionMenuOpen && (
                  <div
                    className="ui-menu fixed bottom-24 right-3 z-50 w-72 rounded-xl border border-[#30363d] bg-[#161b22] p-1.5 shadow-lg sm:absolute sm:bottom-9 sm:right-0 sm:w-80"
                    role="menu"
                    aria-label="Execution mode selection"
                  >
                    <div className="space-y-0.5">
                      {EXECUTION_MODES.map((mode) => {
                        const Icon = mode.icon;
                        const isSelected = executionMode === mode.id;
                        return (
                          <button
                            key={mode.id}
                            type="button"
                            role="menuitem"
                            onClick={() => handleSelectExecutionMode(mode.id)}
                            className={`flex w-full items-start gap-3 rounded-xl p-2.5 text-left transition-colors ${
                              isSelected
                                ? 'bg-[#21262d] text-[#e6edf3]'
                                : 'text-[#8b949e] hover:bg-[#21262d]/60 hover:text-[#e6edf3]'
                            }`}
                          >
                            <div className="mt-0.5 shrink-0">
                              <Icon size={16} className={isSelected ? 'text-[#e6edf3]' : 'text-[#8b949e]'} />
                            </div>
                            <div className="flex-1 min-w-0">
                              <div className="flex items-center justify-between">
                                <span className={`text-xs font-semibold ${isSelected ? 'text-white' : 'text-[#c9d1d9]'}`}>
                                  {mode.title}
                                </span>
                                {isSelected && <Check size={14} className="text-[#e6edf3] shrink-0 ml-2" />}
                              </div>
                              <p className="mt-0.5 text-[11px] leading-relaxed text-[#8b949e]">
                                {mode.description}
                              </p>
                            </div>
                          </button>
                        );
                      })}
                    </div>
                  </div>
                )}
              </div>

              {/* Voice Dictation (Microphone) */}
              <button
                type="button"
                onClick={toggleListening}
                className={`flex h-8 w-8 items-center justify-center rounded-lg border transition-colors ${
                  isListening
                    ? 'border-[#f85149] bg-[#da3633]/25 text-[#f85149]'
                    : 'border-[#30363d] bg-[#21262d] text-[#8b949e] hover:bg-[#30363d] hover:text-[#e6edf3]'
                }`}
                title={isListening ? 'Listening to voice... Click to stop' : 'Voice Dictation (Speech-to-Text)'}
                aria-label="Voice Dictation"
              >
                {isListening ? <MicOff size={13} className="text-[#f85149]" /> : <Mic size={13} />}
              </button>

              <div className="shrink-0">
                {isStreaming ? (
                  <div className="flex items-center gap-2">
                    {((input || '').trim().length > 0 || attachedContexts.length > 0) && (
                      <button
                        type="button"
                        onClick={send}
                        className="flex items-center gap-1.5 rounded-lg border border-[#30363d] bg-[#21262d] px-2.5 py-1.5 text-xs font-medium text-[#58a6ff] hover:bg-[#30363d] hover:border-[#58a6ff]/50 transition-colors"
                        title="Add to queue (executes after current turn)"
                        disabled={isSessionLoading || pendingAttachments > 0}
                        aria-label="Queue message"
                      >
                        <Clock size={12} className="text-[#8b949e]" />
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
                    disabled={runStatus?.phase === 'saving_results' || runStatus?.phase === 'preparing_attachments' || pendingAttachments > 0 || isReviewing || (!(input || '').trim() && attachedContexts.length === 0)}
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
        <button type="button" onClick={() => { invalidateAttachments(); setInput(''); setAttachedContexts([]); setMenuDismissed(false); textareaRef.current?.focus(); }} className="sr-only" aria-label="Clear composer"><X /></button>
      )}
    </div>
  );
}
