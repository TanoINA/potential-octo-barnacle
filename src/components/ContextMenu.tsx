import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  Bot,
  Bug,
  Check,
  ChevronRight,
  Clipboard,
  Code,
  Copy,
  Download,
  ExternalLink,
  FileCode2,
  FileText,
  FolderOpen,
  GitBranch,
  Languages,
  ListCollapse,
  MessageSquare,
  Pencil,
  Play,
  RotateCcw,
  Scissors,
  Settings,
  Sparkles,
  Terminal,
  Trash2,
  Undo2,
} from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import type { ChatMessage } from '../types/agent';

export interface ContextMenuProps {
  workspaceCwd?: string;
  currentSessionId?: string | null;
  onNewSession?: () => void;
  onOpenSettings?: () => void;
  onOpenModelPicker?: () => void;
  onToggleSidebar?: () => void;
  onExportSession?: (format: 'markdown' | 'html' | 'json') => void;
  messages?: ChatMessage[];
  onRetryTurn?: (messageId: string) => Promise<boolean>;
  onEditAndResendMessage?: (messageId: string, newContent: string) => Promise<boolean>;
  onRollbackToMessage?: (messageId: string) => Promise<boolean>;
  onRefreshGitStatus?: () => void;
}

interface MenuItemDef {
  id: string;
  label: string;
  icon?: React.ReactNode;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  action?: () => void | Promise<void>;
  submenu?: MenuItemDef[];
}

interface MenuSectionDef {
  id: string;
  title?: string;
  items: MenuItemDef[];
}

interface MenuState {
  x: number;
  y: number;
  sections: MenuSectionDef[];
}

function insertInputText(input: HTMLInputElement | HTMLTextAreaElement, text: string) {
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? input.value.length;
  const value = input.value.slice(0, start) + text + input.value.slice(end);
  // Bypass React's instance value tracker so its input event reads the change.
  const prototype = input instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(prototype, 'value')?.set?.call(input, value);
  input.dispatchEvent(new Event('input', { bubbles: true }));
  input.focus();
  if (
    input instanceof HTMLTextAreaElement ||
    /^(text|search|url|tel|password)$/i.test(input.type || 'text')
  ) {
    input.setSelectionRange(start + text.length, start + text.length);
  }
}

export const ContextMenu: React.FC<ContextMenuProps> = ({
  workspaceCwd = '',
  currentSessionId,
  onNewSession,
  onOpenSettings,
  onOpenModelPicker,
  onToggleSidebar,
  onExportSession,
  messages = [],
  onRetryTurn,
  onEditAndResendMessage,
  onRollbackToMessage,
  onRefreshGitStatus,
}) => {
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [activeSubmenuId, setActiveSubmenuId] = useState<string | null>(null);
  const [submenuPos, setSubmenuPos] = useState<{ x: number; y: number } | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [focusedIndex, setFocusedIndex] = useState<number>(-1);
  const [focusedSubIndex, setFocusedSubIndex] = useState<number>(-1);

  const menuRef = useRef<HTMLDivElement>(null);
  const submenuRef = useRef<HTMLDivElement>(null);
  const copyTimerRef = useRef<number | null>(null);
  const messagesRef = useRef(messages);
  messagesRef.current = messages;

  useEffect(() => () => {
    if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
  }, []);

  const copyToClipboard = useCallback(async (text: string, id: string) => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
      } else {
        const textarea = document.createElement('textarea');
        textarea.value = text;
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand('copy');
        document.body.removeChild(textarea);
      }
      setCopiedId(id);
      if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
      copyTimerRef.current = window.setTimeout(() => {
        setCopiedId(null);
        setMenu(null);
      }, 350);
    } catch (err) {
      console.error('Failed to copy to clipboard', err);
      setMenu(null);
    }
  }, []);

  const dispatchInsertPrompt = useCallback((text: string, autoSend = false) => {
    window.dispatchEvent(
      new CustomEvent('aether:insert-prompt', {
        detail: { text, autoSend },
      }),
    );
    setMenu(null);
  }, []);

  const closeMenu = useCallback(() => {
    setMenu(null);
    setActiveSubmenuId(null);
    setSubmenuPos(null);
    setFocusedIndex(-1);
    setFocusedSubIndex(-1);
  }, []);

  // Listen to global right-click
  useEffect(() => {
    const handleContextMenu = (e: MouseEvent) => {
      e.preventDefault();

      const target = e.target as HTMLElement | null;
      if (!target) return;

      let selection = '';
      const sel = window.getSelection();
      if (sel) {
        const text = sel.toString().trim();
        if (text) {
          if (typeof sel.getRangeAt === 'function' && sel.rangeCount > 0) {
            try {
              const range = sel.getRangeAt(0);
              if (typeof range.intersectsNode === 'function') {
                if (range.intersectsNode(target)) {
                  selection = text;
                }
              } else if (sel.anchorNode && (target.contains(sel.anchorNode) || target === sel.anchorNode)) {
                selection = text;
              } else {
                selection = text;
              }
            } catch {
              selection = text;
            }
          } else {
            selection = text;
          }
        }
      }

      const isInput =
        target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable;

      const codeBlockEl = target.closest<HTMLElement>('[data-code-block="true"]');
      const fileLinkEl = target.closest<HTMLAnchorElement>('[data-file-link="true"], a[href]');
      const messageEl = target.closest<HTMLElement>('[data-message-id]');

      const rawCode = codeBlockEl?.getAttribute('data-code-content') || '';
      let codeContent = rawCode;
      if (rawCode) {
        try {
          codeContent = decodeURIComponent(rawCode);
        } catch {
          codeContent = rawCode;
        }
      }
      const codeLang = codeBlockEl?.getAttribute('data-code-lang') || '';

      const rawFileHref = fileLinkEl?.getAttribute('data-file-path') || fileLinkEl?.getAttribute('href') || '';
      let filePath = rawFileHref;
      try {
        filePath = decodeURIComponent(rawFileHref);
      } catch {
        // use raw
      }

      const messageId = messageEl?.getAttribute('data-message-id');
      const foundMessage = messageId ? messagesRef.current.find((m) => m.id === messageId) : undefined;

      const sections: MenuSectionDef[] = [];

      // 1. Text Selection Context
      if (selection) {
        sections.push({
          id: 'selection',
          title: 'Selection',
          items: [
            {
              id: 'ask-aether',
              label: 'Ask AetherAgent',
              icon: <Sparkles size={14} className="text-[#a371f7]" />,
              submenu: [
                {
                  id: 'ask-quote',
                  label: 'Quote & Reply',
                  icon: <MessageSquare size={13} className="text-[#58a6ff]" />,
                  action: () => {
                    const quoted = selection
                      .split('\n')
                      .map((l) => `> ${l}`)
                      .join('\n');
                    dispatchInsertPrompt(`${quoted}\n\n`, false);
                  },
                },
                {
                  id: 'ask-explain',
                  label: 'Explain Selection',
                  icon: <Bot size={13} className="text-[#3fb950]" />,
                  action: () => {
                    dispatchInsertPrompt(
                      `Explain the following snippet in detail:\n\n\`\`\`\n${selection}\n\`\`\``,
                      true,
                    );
                  },
                },
                {
                  id: 'ask-refactor',
                  label: 'Refactor & Optimize',
                  icon: <Code size={13} className="text-[#d29922]" />,
                  action: () => {
                    dispatchInsertPrompt(
                      `Refactor and optimize the following code for clarity, robustness, and performance:\n\n\`\`\`\n${selection}\n\`\`\``,
                      true,
                    );
                  },
                },
                {
                  id: 'ask-bugs',
                  label: 'Find Bugs & Security Issues',
                  icon: <Bug size={13} className="text-[#f85149]" />,
                  action: () => {
                    dispatchInsertPrompt(
                      `Find potential bugs, edge cases, and security vulnerabilities in the following:\n\n\`\`\`\n${selection}\n\`\`\``,
                      true,
                    );
                  },
                },
                {
                  id: 'ask-summarize',
                  label: 'TL;DR / Summarize',
                  icon: <ListCollapse size={13} className="text-[#79c0ff]" />,
                  action: () => {
                    dispatchInsertPrompt(
                      `Provide a concise TL;DR summary of the following text:\n\n${selection}`,
                      true,
                    );
                  },
                },
                {
                  id: 'ask-translate-id',
                  label: 'Translate to Indonesian',
                  icon: <Languages size={13} className="text-[#58a6ff]" />,
                  action: () => {
                    dispatchInsertPrompt(
                      `Terjemahkan teks berikut ke bahasa Indonesia yang jelas dan natural:\n\n${selection}`,
                      true,
                    );
                  },
                },
                {
                  id: 'ask-translate-en',
                  label: 'Translate to English',
                  icon: <Languages size={13} className="text-[#58a6ff]" />,
                  action: () => {
                    dispatchInsertPrompt(
                      `Translate the following text into fluent, professional English:\n\n${selection}`,
                      true,
                    );
                  },
                },
              ],
            },
            {
              id: 'sel-copy',
              label: 'Copy',
              icon: <Copy size={14} className="text-[#8b949e]" />,
              shortcut: 'Ctrl+C',
              action: () => copyToClipboard(selection, 'sel-copy'),
            },
            {
              id: 'sel-copy-quote',
              label: 'Copy as Quote',
              icon: <MessageSquare size={14} className="text-[#8b949e]" />,
              action: () => {
                const quoted = selection
                  .split('\n')
                  .map((l) => `> ${l}`)
                  .join('\n');
                void copyToClipboard(quoted, 'sel-copy-quote');
              },
            },
            {
              id: 'sel-run-task',
              label: 'Run in PowerShell as Task',
              icon: <Play size={14} className="text-[#3fb950]" />,
              action: () => {
                dispatchInsertPrompt(
                  `Run the following command as a background task:\n\`\`\`powershell\n${selection}\n\`\`\``,
                  true,
                );
              },
            },
          ],
        });
      }

      // 2. Input / Textarea Context
      if (isInput) {
        const inputEl = target as HTMLInputElement | HTMLTextAreaElement;
        const hasSelection = (inputEl.selectionEnd ?? 0) > (inputEl.selectionStart ?? 0);
        sections.push({
          id: 'input-actions',
          title: 'Input',
          items: [
            ...(hasSelection
              ? [
                  {
                    id: 'input-cut',
                    label: 'Cut',
                    icon: <Scissors size={14} className="text-[#8b949e]" />,
                    shortcut: 'Ctrl+X',
                    action: () => {
                      document.execCommand('cut');
                      closeMenu();
                    },
                  },
                  {
                    id: 'input-copy',
                    label: 'Copy',
                    icon: <Copy size={14} className="text-[#8b949e]" />,
                    shortcut: 'Ctrl+C',
                    action: () => {
                      document.execCommand('copy');
                      closeMenu();
                    },
                  },
                ]
              : []),
            {
              id: 'input-paste',
              label: 'Paste',
              icon: <Clipboard size={14} className="text-[#8b949e]" />,
              shortcut: 'Ctrl+V',
              action: async () => {
                try {
                  const clip = await navigator.clipboard.readText();
                  if (clip) {
                    insertInputText(inputEl, clip);
                  }
                } catch {
                  document.execCommand('paste');
                }
                closeMenu();
              },
            },
            {
              id: 'input-paste-code',
              label: 'Paste as Code Block',
              icon: <FileCode2 size={14} className="text-[#58a6ff]" />,
              action: async () => {
                try {
                  const clip = await navigator.clipboard.readText();
                  if (clip) {
                    const snippet = `\n\`\`\`\n${clip}\n\`\`\`\n`;
                    insertInputText(inputEl, snippet);
                  }
                } catch (e) {
                  console.error('Failed to paste code block', e);
                }
                closeMenu();
              },
            },
            {
              id: 'input-select-all',
              label: 'Select All',
              shortcut: 'Ctrl+A',
              action: () => {
                inputEl.select();
                closeMenu();
              },
            },
            ...(inputEl.getAttribute('aria-label') === 'Message AetherAgent'
              ? [
                  {
                    id: 'input-clear',
                    label: 'Clear Composer',
                    icon: <Trash2 size={14} className="text-[#f85149]" />,
                    danger: true,
                    action: () => {
                      window.dispatchEvent(new CustomEvent('aether:clear-composer'));
                      closeMenu();
                    },
                  },
                ]
              : []),
          ],
        });
      }

      // 3. Code Block Context
      if (codeContent) {
        sections.push({
          id: 'code-block',
          title: codeLang ? `Code (${codeLang})` : 'Code Block',
          items: [
            {
              id: 'code-copy',
              label: 'Copy Code',
              icon: <Copy size={14} className="text-[#8b949e]" />,
              action: () => copyToClipboard(codeContent, 'code-copy'),
            },
            {
              id: 'code-explain',
              label: 'Explain Architecture & Flow',
              icon: <Bot size={14} className="text-[#3fb950]" />,
              action: () => {
                dispatchInsertPrompt(
                  `Explain the architecture, algorithms, and execution flow of this ${codeLang || 'code'}:\n\n\`\`\`${codeLang}\n${codeContent}\n\`\`\``,
                  true,
                );
              },
            },
            {
              id: 'code-tests',
              label: 'Generate Unit Tests',
              icon: <Sparkles size={14} className="text-[#a371f7]" />,
              action: () => {
                dispatchInsertPrompt(
                  `Generate thorough unit tests for this ${codeLang || 'code'}:\n\n\`\`\`${codeLang}\n${codeContent}\n\`\`\``,
                  true,
                );
              },
            },
            {
              id: 'code-bugs',
              label: 'Audit Bugs & Edge Cases',
              icon: <Bug size={14} className="text-[#f85149]" />,
              action: () => {
                dispatchInsertPrompt(
                  `Perform a rigorous security and correctness audit of this ${codeLang || 'code'}:\n\n\`\`\`${codeLang}\n${codeContent}\n\`\`\``,
                  true,
                );
              },
            },
            {
              id: 'code-save',
              label: 'Save Code to File…',
              icon: <FileCode2 size={14} className="text-[#58a6ff]" />,
              action: () => {
                dispatchInsertPrompt(
                  `Save this code into an appropriate file in the workspace:\n\n\`\`\`${codeLang}\n${codeContent}\n\`\`\``,
                  false,
                );
              },
            },
          ],
        });
      }

      // 4. File Link / Artifact Context
      if (filePath) {
        const isWeb = /^(https?:\/\/|mailto:)/i.test(filePath);
        sections.push({
          id: 'file-link',
          title: isWeb ? 'Web Link' : 'File / Artifact',
          items: [
            {
              id: 'file-open',
              label: isWeb ? 'Open in Browser' : 'Open File',
              icon: isWeb ? <ExternalLink size={14} className="text-[#58a6ff]" /> : <FileText size={14} className="text-[#a371f7]" />,
              action: () => {
                if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
                  void Promise.resolve(invoke('open_path_or_url', { pathOrUrl: filePath })).catch(() => undefined);
                } else if (isWeb) {
                  window.open(filePath, '_blank', 'noopener,noreferrer');
                }
                closeMenu();
              },
            },
            ...(!isWeb
              ? [
                  {
                    id: 'file-reveal',
                    label: 'Reveal in File Explorer',
                    icon: <FolderOpen size={14} className="text-[#d29922]" />,
                    action: () => {
                      if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
                        void Promise.resolve(invoke('reveal_in_explorer', { path: filePath })).catch(() => undefined);
                      }
                      closeMenu();
                    },
                  },
                  {
                    id: 'file-terminal',
                    label: 'Open Terminal in Folder',
                    icon: <Terminal size={14} className="text-[#3fb950]" />,
                    action: () => {
                      if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
                        void Promise.resolve(invoke('open_terminal', { path: filePath })).catch(() => undefined);
                      }
                      closeMenu();
                    },
                  },
                ]
              : []),
            {
              id: 'file-copy-path',
              label: isWeb ? 'Copy Link URL' : 'Copy File Path',
              icon: <Copy size={14} className="text-[#8b949e]" />,
              action: () => copyToClipboard(filePath, 'file-copy-path'),
            },
            {
              id: 'file-insert-prompt',
              label: 'Insert Path to Composer',
              icon: <MessageSquare size={14} className="text-[#58a6ff]" />,
              action: () => dispatchInsertPrompt(filePath, false),
            },
          ],
        });
      }

      // 5. Message Row Context
      if (foundMessage) {
        const isMsgUser = foundMessage.role === 'user';
        const msgText = [foundMessage.preamble, foundMessage.content].filter(Boolean).join('\n\n');
        sections.push({
          id: 'message-row',
          title: isMsgUser ? 'User Message' : 'Assistant Turn',
          items: [
            ...(msgText
              ? [
                  {
                    id: 'msg-copy',
                    label: 'Copy Message',
                    icon: <Copy size={14} className="text-[#8b949e]" />,
                    action: () => copyToClipboard(msgText, 'msg-copy'),
                  },
                  {
                    id: 'msg-copy-json',
                    label: 'Copy Raw Turn JSON',
                    icon: <FileCode2 size={14} className="text-[#8b949e]" />,
                    action: () => copyToClipboard(JSON.stringify(foundMessage, null, 2), 'msg-copy-json'),
                  },
                ]
              : []),
            ...(isMsgUser && onEditAndResendMessage
              ? [
                  {
                    id: 'msg-edit',
                    label: 'Edit & Resend from Here',
                    icon: <Pencil size={14} className="text-[#58a6ff]" />,
                    action: () => {
                      messageEl?.querySelector<HTMLButtonElement>('[aria-label="Edit message"]')?.click();
                      closeMenu();
                    },
                  },
                ]
              : []),
            ...(!isMsgUser && onRetryTurn
              ? [
                  {
                    id: 'msg-regenerate',
                    label: 'Regenerate Response',
                    icon: <RotateCcw size={14} className="text-[#58a6ff]" />,
                    action: () => {
                      void onRetryTurn(foundMessage.id);
                      closeMenu();
                    },
                  },
                ]
              : []),
            ...(onRollbackToMessage
              ? [
                  {
                    id: 'msg-rollback',
                    label: 'Rollback to this Turn',
                    icon: <Undo2 size={14} className="text-[#f85149]" />,
                    danger: true,
                    action: () => {
                      void onRollbackToMessage(foundMessage.id);
                      closeMenu();
                    },
                  },
                ]
              : []),
          ],
        });
      }

      // 6. Global / Workspace / Canvas Section
      sections.push({
        id: 'workspace-global',
        title: 'AetherAgent',
        items: [
          ...(onNewSession
            ? [
                {
                  id: 'global-new-chat',
                  label: 'New Chat',
                  icon: <MessageSquare size={14} className="text-[#58a6ff]" />,
                  shortcut: 'Ctrl+N',
                  action: () => {
                    onNewSession();
                    closeMenu();
                  },
                },
              ]
            : []),
          {
            id: 'global-open-explorer',
            label: 'Open Workspace in Explorer',
            icon: <FolderOpen size={14} className="text-[#d29922]" />,
            disabled: !workspaceCwd,
            action: () => {
              if (workspaceCwd && typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
                void Promise.resolve(invoke('reveal_in_explorer', { path: workspaceCwd })).catch(() => undefined);
              }
              closeMenu();
            },
          },
          {
            id: 'global-open-terminal',
            label: 'Open Terminal in Workspace',
            icon: <Terminal size={14} className="text-[#3fb950]" />,
            action: () => {
              if (typeof window !== 'undefined' && (window as any).__TAURI_INTERNALS__) {
                void Promise.resolve(invoke('open_terminal', { path: workspaceCwd || undefined })).catch(() => undefined);
              }
              closeMenu();
            },
          },
          ...(onRefreshGitStatus
            ? [
                {
                  id: 'global-git-refresh',
                  label: 'Refresh Git Status',
                  icon: <GitBranch size={14} className="text-[#8b949e]" />,
                  action: () => {
                    onRefreshGitStatus();
                    closeMenu();
                  },
                },
              ]
            : []),
          ...(onExportSession && currentSessionId
            ? [
                {
                  id: 'global-export',
                  label: 'Export Session',
                  icon: <Download size={14} className="text-[#8b949e]" />,
                  submenu: [
                    {
                      id: 'export-md',
                      label: 'Export as Markdown (.md)',
                      icon: <FileText size={13} className="text-[#58a6ff]" />,
                      action: () => {
                        onExportSession('markdown');
                        closeMenu();
                      },
                    },
                    {
                      id: 'export-html',
                      label: 'Export as HTML (.html)',
                      icon: <Code size={13} className="text-[#3fb950]" />,
                      action: () => {
                        onExportSession('html');
                        closeMenu();
                      },
                    },
                    {
                      id: 'export-json',
                      label: 'Export as Raw JSON (.json)',
                      icon: <FileCode2 size={13} className="text-[#a371f7]" />,
                      action: () => {
                        onExportSession('json');
                        closeMenu();
                      },
                    },
                  ],
                },
              ]
            : []),
          ...(onOpenModelPicker
            ? [
                {
                  id: 'global-model-picker',
                  label: 'Change Model',
                  icon: <Bot size={14} className="text-[#8b949e]" />,
                  shortcut: 'Ctrl+K',
                  action: () => {
                    onOpenModelPicker();
                    closeMenu();
                  },
                },
              ]
            : []),
          ...(onOpenSettings
            ? [
                {
                  id: 'global-settings',
                  label: 'Settings',
                  icon: <Settings size={14} className="text-[#8b949e]" />,
                  shortcut: 'Ctrl+,',
                  action: () => {
                    onOpenSettings();
                    closeMenu();
                  },
                },
              ]
            : []),
        ],
      });

      // Clamp initial position to avoid overflow
      const clampedX = Math.max(8, Math.min(e.clientX, window.innerWidth - 250));
      const clampedY = Math.max(8, Math.min(e.clientY, window.innerHeight - 340));

      setMenu({
        x: clampedX,
        y: clampedY,
        sections,
      });
      setActiveSubmenuId(null);
      setSubmenuPos(null);
      setFocusedIndex(-1);
      setFocusedSubIndex(-1);
    };

    window.addEventListener('contextmenu', handleContextMenu);
    return () => window.removeEventListener('contextmenu', handleContextMenu);
  }, [
    workspaceCwd,
    currentSessionId,
    onNewSession,
    onOpenSettings,
    onOpenModelPicker,
    onToggleSidebar,
    onExportSession,
    onRetryTurn,
    onEditAndResendMessage,
    onRollbackToMessage,
    onRefreshGitStatus,
    copyToClipboard,
    dispatchInsertPrompt,
    closeMenu,
  ]);

  const flatItems = useMemo(() => {
    if (!menu) return [];
    return menu.sections.flatMap((s) => s.items).filter((it) => !it.disabled);
  }, [menu]);

  const activeSubmenuItem = menu?.sections
    .flatMap((s) => s.items)
    .find((it) => it.id === activeSubmenuId);

  const flatSubItems = useMemo(() => {
    if (!activeSubmenuItem?.submenu) return [];
    return activeSubmenuItem.submenu.filter((it) => !it.disabled);
  }, [activeSubmenuItem]);

  // Close on outside click, window blur, or scroll, and handle keyboard navigation
  useEffect(() => {
    if (!menu) return;

    const handleClickOutside = (e: MouseEvent) => {
      if (
        menuRef.current &&
        !menuRef.current.contains(e.target as Node) &&
        (!submenuRef.current || !submenuRef.current.contains(e.target as Node))
      ) {
        closeMenu();
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      // The composer can retain focus after right-clicking. Intercept menu keys
      // in capture before its React handlers can submit or edit the draft.
      if (['Escape', 'ArrowDown', 'ArrowUp', 'ArrowRight', 'ArrowLeft', 'Enter'].includes(e.key)) {
        e.stopPropagation();
      }

      if (e.key === 'Escape') {
        e.preventDefault();
        if (activeSubmenuId) {
          setActiveSubmenuId(null);
          setSubmenuPos(null);
          setFocusedSubIndex(-1);
        } else {
          closeMenu();
        }
        return;
      }

      if (e.key === 'ArrowDown') {
        e.preventDefault();
        if (activeSubmenuId && flatSubItems.length > 0) {
          setFocusedSubIndex((prev) => (prev + 1) % flatSubItems.length);
        } else if (flatItems.length > 0) {
          setFocusedIndex((prev) => (prev + 1) % flatItems.length);
        }
        return;
      }

      if (e.key === 'ArrowUp') {
        e.preventDefault();
        if (activeSubmenuId && flatSubItems.length > 0) {
          setFocusedSubIndex((prev) => (prev - 1 + flatSubItems.length) % flatSubItems.length);
        } else if (flatItems.length > 0) {
          setFocusedIndex((prev) => (prev - 1 + flatItems.length) % flatItems.length);
        }
        return;
      }

      if (e.key === 'ArrowRight') {
        if (!activeSubmenuId && focusedIndex >= 0 && flatItems[focusedIndex]?.submenu?.length) {
          e.preventDefault();
          const item = flatItems[focusedIndex];
          const buttons = menuRef.current?.querySelectorAll<HTMLButtonElement>('button:not([disabled])');
          const btn = buttons ? buttons[focusedIndex] : null;
          if (btn) {
            const rect = btn.getBoundingClientRect();
            const submenuWidth = 230;
            let x = rect.right + 4;
            if (x + submenuWidth > window.innerWidth - 8) {
              x = rect.left - submenuWidth - 4;
            }
            let y = rect.top;
            if (y + 240 > window.innerHeight - 8) {
              y = Math.max(8, window.innerHeight - 250);
            }
            setActiveSubmenuId(item.id);
            setSubmenuPos({ x, y });
            setFocusedSubIndex(0);
          } else {
            setActiveSubmenuId(item.id);
            setSubmenuPos({ x: (menu?.x ?? 0) + 200, y: menu?.y ?? 0 });
            setFocusedSubIndex(0);
          }
        }
        return;
      }

      if (e.key === 'ArrowLeft') {
        if (activeSubmenuId) {
          e.preventDefault();
          setActiveSubmenuId(null);
          setSubmenuPos(null);
          setFocusedSubIndex(-1);
        }
        return;
      }

      if (e.key === 'Enter') {
        e.preventDefault();
        if (activeSubmenuId && focusedSubIndex >= 0 && flatSubItems[focusedSubIndex]) {
          const sub = flatSubItems[focusedSubIndex];
          if (sub.action) {
            void sub.action();
          }
        } else if (focusedIndex >= 0 && flatItems[focusedIndex]) {
          const item = flatItems[focusedIndex];
          if (item.submenu && item.submenu.length > 0) {
            setActiveSubmenuId(item.id);
            const buttons = menuRef.current?.querySelectorAll<HTMLButtonElement>('button:not([disabled])');
            const btn = buttons ? buttons[focusedIndex] : null;
            if (btn) {
              const rect = btn.getBoundingClientRect();
              setSubmenuPos({ x: rect.right + 4, y: rect.top });
            } else {
              setSubmenuPos({ x: (menu?.x ?? 0) + 200, y: menu?.y ?? 0 });
            }
            setFocusedSubIndex(0);
          } else if (item.action) {
            void item.action();
          }
        }
        return;
      }
    };

    const handleScroll = (e: Event) => {
      const target = e.target;
      if (
        target instanceof Node &&
        (menuRef.current?.contains(target) || submenuRef.current?.contains(target))
      ) {
        return;
      }
      closeMenu();
    };

    window.addEventListener('mousedown', handleClickOutside);
    window.addEventListener('keydown', handleKeyDown, true);
    window.addEventListener('scroll', handleScroll, true);
    window.addEventListener('blur', closeMenu);

    return () => {
      window.removeEventListener('mousedown', handleClickOutside);
      window.removeEventListener('keydown', handleKeyDown, true);
      window.removeEventListener('scroll', handleScroll, true);
      window.removeEventListener('blur', closeMenu);
    };
  }, [menu, closeMenu, activeSubmenuId, flatItems, flatSubItems, focusedIndex, focusedSubIndex]);

  // Adjust menu position after layout render
  useLayoutEffect(() => {
    if (!menu || !menuRef.current) return;
    const rect = menuRef.current.getBoundingClientRect();
    let newX = menu.x;
    let newY = menu.y;
    let adjusted = false;

    if (rect.right > window.innerWidth - 8) {
      newX = Math.max(8, window.innerWidth - rect.width - 8);
      adjusted = true;
    }
    if (rect.bottom > window.innerHeight - 8) {
      newY = Math.max(8, window.innerHeight - rect.height - 8);
      adjusted = true;
    }
    if (adjusted) {
      setMenu((prev) => (prev ? { ...prev, x: newX, y: newY } : null));
    }
  }, [menu?.sections]);

  const handleItemHover = (item: MenuItemDef, e: React.MouseEvent<HTMLButtonElement>) => {
    if (item.submenu && item.submenu.length > 0) {
      const rect = e.currentTarget.getBoundingClientRect();
      const submenuWidth = 230;
      let x = rect.right + 4;
      if (x + submenuWidth > window.innerWidth - 8) {
        x = rect.left - submenuWidth - 4;
      }
      let y = rect.top;
      if (y + 240 > window.innerHeight - 8) {
        y = Math.max(8, window.innerHeight - 250);
      }
      setActiveSubmenuId(item.id);
      setSubmenuPos({ x, y });
    } else {
      setActiveSubmenuId(null);
      setSubmenuPos(null);
    }
  };

  if (!menu) return null;

  return (
    <>
      <div
        ref={menuRef}
        data-testid="aether-context-menu"
        style={{ top: `${menu.y}px`, left: `${menu.x}px` }}
        className="ui-menu ui-panel-enter fixed z-50 min-w-[210px] max-w-[280px] max-h-[80vh] overflow-y-auto rounded-xl border border-[#30363d] bg-[#161b22]/95 p-1 text-[12px] text-[#e6edf3] shadow-lg select-none"
      >
        {menu.sections.map((section, sIdx) => (
          <div key={section.id}>
            {sIdx > 0 && <div className="h-px bg-[#30363d]/70 my-1 mx-1.5" />}
            {section.title && (
              <div className="px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider text-[#6e7681]">
                {section.title}
              </div>
            )}
            <div className="space-y-0.5">
              {section.items.map((item) => {
                const isCopied = copiedId === item.id;
                const hasSubmenu = Boolean(item.submenu && item.submenu.length > 0);
                const isActiveSub = activeSubmenuId === item.id;
                const isFocused = !activeSubmenuId && flatItems[focusedIndex]?.id === item.id;

                return (
                  <button
                    key={item.id}
                    type="button"
                    disabled={item.disabled}
                    onClick={() => {
                      if (!hasSubmenu && item.action) {
                        void item.action();
                      }
                    }}
                    onMouseEnter={(e) => {
                      setFocusedIndex(flatItems.findIndex((it) => it.id === item.id));
                      handleItemHover(item, e);
                    }}
                    className={`group flex w-full items-center justify-between gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-xs transition-colors ${
                      item.disabled
                        ? 'cursor-not-allowed opacity-40'
                        : item.danger
                        ? isFocused
                          ? 'bg-[#b62324]/30 text-[#ff7b72]'
                          : 'text-[#f85149] hover:bg-[#b62324]/20 hover:text-[#ff7b72]'
                        : isActiveSub || isFocused
                        ? 'bg-[#21262d] text-white'
                        : 'text-[#c9d1d9] hover:bg-[#21262d] hover:text-white'
                    }`}
                  >
                    <div className="flex items-center gap-2 min-w-0">
                      {isCopied ? (
                        <Check size={14} className="text-[#3fb950] shrink-0" />
                      ) : (
                        item.icon && <span className="shrink-0">{item.icon}</span>
                      )}
                      <span className="truncate">{item.label}</span>
                    </div>

                    <div className="flex items-center gap-1.5 pl-2 text-[10px] text-[#6e7681]">
                      {isCopied && <span className="text-[#3fb950] font-medium">Copied!</span>}
                      {item.shortcut && !isCopied && (
                        <kbd className="rounded border border-[#30363d] bg-[#0d1117] px-1 py-0.5 font-mono text-[9px] text-[#8b949e]">
                          {item.shortcut}
                        </kbd>
                      )}
                      {hasSubmenu && <ChevronRight size={13} className="text-[#8b949e]" />}
                    </div>
                  </button>
                );
              })}
            </div>
          </div>
        ))}
      </div>

      {/* Nested Submenu */}
      {activeSubmenuItem && submenuPos && activeSubmenuItem.submenu && (
        <div
          ref={submenuRef}
          data-testid="aether-context-submenu"
          style={{ top: `${submenuPos.y}px`, left: `${submenuPos.x}px` }}
          className="ui-menu ui-panel-enter fixed z-50 min-w-[210px] max-w-[280px] max-h-[80vh] overflow-y-auto rounded-xl border border-[#30363d] bg-[#161b22]/95 p-1 text-[12px] text-[#e6edf3] shadow-lg select-none"
        >
          <div className="px-2.5 py-1 text-[10px] font-semibold uppercase tracking-wider text-[#6e7681]">
            {activeSubmenuItem.label}
          </div>
          <div className="space-y-0.5">
            {activeSubmenuItem.submenu.map((sub) => {
              const isSubFocused = flatSubItems[focusedSubIndex]?.id === sub.id;

              return (
                <button
                  key={sub.id}
                  type="button"
                  disabled={sub.disabled}
                  onClick={() => {
                    if (sub.action) void sub.action();
                  }}
                  onMouseEnter={() =>
                    setFocusedSubIndex(flatSubItems.findIndex((it) => it.id === sub.id))
                  }
                  className={`group flex w-full items-center justify-between gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-xs transition-colors ${
                    sub.disabled
                      ? 'cursor-not-allowed opacity-40'
                      : sub.danger
                      ? isSubFocused
                        ? 'bg-[#b62324]/30 text-[#ff7b72]'
                        : 'text-[#f85149] hover:bg-[#b62324]/20 hover:text-[#ff7b72]'
                      : isSubFocused
                      ? 'bg-[#21262d] text-white'
                      : 'text-[#c9d1d9] hover:bg-[#21262d] hover:text-white'
                  }`}
                >
                <div className="flex items-center gap-2 min-w-0">
                  {sub.icon && <span className="shrink-0">{sub.icon}</span>}
                  <span className="truncate">{sub.label}</span>
                </div>
                {sub.shortcut && (
                  <kbd className="rounded border border-[#30363d] bg-[#0d1117] px-1 py-0.5 font-mono text-[9px] text-[#8b949e]">
                    {sub.shortcut}
                  </kbd>
                )}
              </button>
            );
          })}
          </div>
        </div>
      )}
    </>
  );
};
