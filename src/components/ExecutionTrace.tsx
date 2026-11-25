import React, { useState, useMemo, useEffect, useId, useRef } from 'react';
import {
  Ban,
  BookOpen,
  Bot,
  ChevronDown,
  ChevronRight,
  ExternalLink,
  FileCode2,
  FileText,
  Globe,
  HelpCircle,
  Loader2,
  Search,
  Settings,
  Terminal,
} from 'lucide-react';
import type { ToolCallItem } from '../types/agent';
import { safeEncodeURIComponent } from '../utils/encoding';
import { TerminalOutput } from './TerminalOutput';

interface ExecutionTraceProps {
  toolCalls: ToolCallItem[];
  isStreaming?: boolean;
  onSelectArtifact?: (id: string) => void;
}

export function parseSearchLinks(text?: string): { title: string; url: string; snippet?: string }[] {
  if (!text) return [];
  const matches = [...text.matchAll(/\d+\.\s*\[(.*?)\]\((.*?)\)(?:\s*\n\s*(.*?)(?=\n\n|\n\d+\.|$))?/gs)];
  const safe: { title: string; url: string; snippet?: string }[] = [];
  for (const m of matches) {
    try {
      const parsed = new URL(m[2]);
      if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') continue;
      safe.push({
        title: m[1],
        url: m[2],
        snippet: m[3]?.trim(),
      });
    } catch {
      continue;
    }
  }
  return safe;
}

function formatDuration(ms?: number): string {
  if (ms === undefined || ms === null) return '';
  if (ms < 1000) return `${ms}ms`;
  const seconds = (ms / 1000).toFixed(1);
  return `${seconds.endsWith('.0') ? seconds.slice(0, -2) : seconds}s`;
}

function isToolFailed(tool: ToolCallItem): boolean {
  return tool.status !== 'cancelled' && (tool.status === 'error' || (tool.exitCode != null && tool.exitCode !== 0));
}

function subagentScope(id: string): string | undefined {
  const slash = id.indexOf('/');
  return id.startsWith('sub-') && slash > 4 ? id.slice(0, slash) : undefined;
}

function extractArg(tool: ToolCallItem, key: string): string {
  if (tool.args && typeof tool.args[key] === 'string' && tool.args[key].trim()) {
    return tool.args[key].trim();
  }
  if (tool.argumentsRaw) {
    try {
      const parsed = JSON.parse(tool.argumentsRaw);
      if (typeof parsed[key] === 'string' && parsed[key].trim()) {
        return parsed[key].trim();
      }
    } catch {
      // Partial JSON during streaming, e.g. {"command":"Get-ChildItem...
      const regex = new RegExp(`"${key}"\\s*:\\s*"([^"\\\\]*(?:\\\\.[^"\\\\]*)*)`);
      const match = tool.argumentsRaw.match(regex);
      if (match && match[1]) {
        try {
          return JSON.parse(`"${match[1]}"`).trim();
        } catch {
          return match[1].replace(/\\n/g, ' ').trim();
        }
      }
    }
  }
  return '';
}

function getToolDisplayInfo(tool: ToolCallItem): {
  icon: React.ReactNode;
  label: string;
  isTerminal: boolean;
  command?: string;
  output?: string;
  isRunning: boolean;
  filePath?: string;
} {
  const name = tool.name;
  const isRunning = tool.status === 'running' || tool.status === 'pending';

  if (name === 'use_skill' || (name === 'read_file' && String(tool.args?.path || tool.args?.filePath || '').endsWith('SKILL.md'))) {
    const skillName = extractArg(tool, 'name') || String(tool.args?.path || tool.args?.filePath || '').replace(/.*[/\\]([^/\\]+)[/\\]SKILL\.md$/, '$1') || 'skill';
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#f0883e] animate-spin shrink-0" /> : <BookOpen size={13} className="text-[#f0883e] shrink-0" />,
      label: isRunning ? `Loading skill: ${skillName}...` : `Loaded skill: ${skillName}`,
      isTerminal: false,
      isRunning,
    };
  }

  if (name === 'run_command') {
    const command = extractArg(tool, 'command');
    const isDaemon = tool.args?.isDaemon === true || tool.args?.is_daemon === true;
    const summary = extractArg(tool, 'toolSummary') || extractArg(tool, 'tool_summary');
    const displayCmd = summary || (command.length > 70 ? `${command.slice(0, 67)}...` : command);
    let label = '';
    if (isRunning) {
      label = isDaemon
        ? (displayCmd ? `Starting background task "${displayCmd}"...` : 'Starting background task...')
        : (displayCmd ? `Running "${displayCmd}"` : 'Running command...');
    } else {
      label = isDaemon
        ? (displayCmd ? `Started background task "${displayCmd}"` : 'Started background task')
        : (displayCmd ? `Ran "${displayCmd}"` : 'Ran command');
    }
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#f0883e] animate-spin shrink-0" /> : <Settings size={13} className="text-[#8b949e] shrink-0" />,
      label,
      isTerminal: true,
      command,
      output: tool.terminalOutput || tool.result || '',
      isRunning,
    };
  }

  if (name === 'manage_task') {
    const action = extractArg(tool, 'action') || 'manage';
    const taskId = extractArg(tool, 'taskId') || extractArg(tool, 'task_id') || '';
    let label = '';
    if (action === 'list') {
      label = isRunning ? 'Listing background tasks...' : 'Listed background tasks';
    } else if (action === 'status') {
      label = isRunning ? `Checking status for ${taskId || 'task'}...` : `Checked status for ${taskId || 'task'}`;
    } else if (action === 'send_input') {
      label = isRunning ? `Sending input to ${taskId || 'task'}...` : `Sent input to ${taskId || 'task'}`;
    } else if (action === 'kill') {
      label = isRunning ? `Terminating ${taskId || 'task'}...` : `Terminated ${taskId || 'task'}`;
    } else {
      label = isRunning ? `Managing task ${taskId}...` : `Managed task ${taskId}`;
    }
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#f0883e] animate-spin shrink-0" /> : <Terminal size={13} className="text-[#58a6ff] shrink-0" />,
      label,
      isTerminal: true,
      command: `manage_task ${action}${taskId ? ` ${taskId}` : ''}`,
      output: tool.terminalOutput || tool.result || '',
      isRunning,
    };
  }

  if (name === 'read_file') {
    const rawPath = extractArg(tool, 'Path') || extractArg(tool, 'path') || extractArg(tool, 'filePath') || '';
    const filename = rawPath.replace(/.*[/\\]/, '') || 'file';
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <FileText size={13} className="text-[#8b949e] shrink-0" />,
      label: isRunning ? `Reading ${filename}...` : `Read ${filename}`,
      isTerminal: false,
      isRunning,
      filePath: rawPath || undefined,
    };
  }

  if (name === 'search_files') {
    const pattern = extractArg(tool, 'pattern');
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <Search size={13} className="text-[#8b949e] shrink-0" />,
      label: isRunning ? (pattern ? `Searching files "${pattern}"...` : 'Searching files...') : (pattern ? `Searched files "${pattern}"` : 'Searched files'),
      isTerminal: false,
      isRunning,
    };
  }

  if (name === 'list_directory') {
    const path = extractArg(tool, 'Path') || extractArg(tool, 'path') || '.';
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <FileCode2 size={13} className="text-[#8b949e] shrink-0" />,
      label: isRunning ? `Exploring directory "${path}"...` : `Explored directory "${path}"`,
      isTerminal: false,
      isRunning,
      filePath: path !== '.' ? path : undefined,
    };
  }

  if (name === 'edit_file' || name === 'write_file') {
    const rawPath = extractArg(tool, 'Path') || extractArg(tool, 'path') || extractArg(tool, 'filePath') || '';
    const filename = rawPath.replace(/.*[/\\]/, '') || 'file';
    const action = name === 'edit_file' ? 'Edit' : 'Write';
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <FileCode2 size={13} className="text-[#58a6ff] shrink-0" />,
      label: isRunning ? `${action}ing ${filename}...` : `${name === 'edit_file' ? 'Edited' : 'Wrote'} ${filename}`,
      isTerminal: false,
      isRunning,
      filePath: rawPath || undefined,
    };
  }

  if (name === 'ask_question') {
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <HelpCircle size={13} className="text-[#58a6ff] shrink-0" />,
      label: isRunning ? 'Asking question...' : 'Asked clarifying question',
      isTerminal: false,
      isRunning,
    };
  }

  if (name === 'search_web') {
    const query = extractArg(tool, 'query');
    const displayQuery = query.length > 35 ? `${query.slice(0, 32)}...` : query;
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <Search size={13} className="text-[#58a6ff] shrink-0" />,
      label: isRunning ? (displayQuery ? `Searching web for "${displayQuery}"...` : 'Searching web...') : (displayQuery ? `Searched web for "${displayQuery}"` : 'Searched web'),
      isTerminal: false,
      isRunning,
    };
  }

  if (name === 'fetch_web_page') {
    const url = extractArg(tool, 'url');
    let host = url;
    try {
      if (url) host = new URL(url).hostname;
    } catch {
      // ignore
    }
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <Globe size={13} className="text-[#58a6ff] shrink-0" />,
      label: isRunning ? (host ? `Fetching ${host}...` : 'Fetching web page...') : (host ? `Fetched ${host}` : 'Fetched web page'),
      isTerminal: false,
      isRunning,
    };
  }

  if (name === 'spawn_subagent') {
    const role = extractArg(tool, 'role') || extractArg(tool, 'task').slice(0, 30) || 'Subagent';
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <Bot size={13} className="text-[#58a6ff] shrink-0" />,
      label: isRunning ? `Spawning subagent: ${role}...` : `spawn_subagent: ${role}`,
      isTerminal: false,
      isRunning,
    };
  }

  if (name === 'learn_skill') {
    const skillName = extractArg(tool, 'name') || 'skill';
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#f0883e] animate-spin shrink-0" /> : <BookOpen size={13} className="text-[#f0883e] shrink-0" />,
      label: isRunning ? `Learning skill: ${skillName}...` : `Learned skill: ${skillName}`,
      isTerminal: false,
      isRunning,
    };
  }

  return {
    icon: isRunning ? <Loader2 size={13} className="text-[#8b949e] animate-spin shrink-0" /> : <Settings size={13} className="text-[#8b949e] shrink-0" />,
    label: isRunning ? `Executing ${name}...` : `${name}`,
    isTerminal: false,
    isRunning,
  };
}

// Jam lokal hanya hidup selama baris aktif ditampilkan.
const ActiveToolDuration: React.FC<{ startedAt?: number }> = ({ startedAt }) => {
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const interval = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(interval);
  }, []);

  const elapsedSec = Math.max(1, Math.floor((now - (startedAt || now)) / 1000));

  return (
    <span className="flex items-center gap-1.5 rounded bg-[var(--ui-orange-bg)] px-1.5 py-0.5 text-[10px] font-mono font-medium text-[var(--ui-orange)] border border-[var(--ui-orange-border)] animate-pulse">
      <Loader2 size={10} className="animate-spin text-[var(--ui-orange)]" />
      <span>{elapsedSec}s</span>
    </span>
  );
};

export const ExecutionTrace: React.FC<ExecutionTraceProps> = ({
  toolCalls,
  isStreaming = false,
  onSelectArtifact,
}) => {
  const [manualCollapsed, setManualCollapsed] = useState<boolean | null>(null);
  const [expandedDetails, setExpandedDetails] = useState<Record<string, boolean>>({});
  const traceId = useId();
  // Pembaruan tool mengganti objeknya; metadata lama tidak perlu diurai ulang.
  const displayInfoCache = useRef(new WeakMap<ToolCallItem, ReturnType<typeof getToolDisplayInfo>>());

  const [expandedSubagents, setExpandedSubagents] = useState<Record<string, boolean>>({});
  const { mainTools, subagents, failureCount } = useMemo(() => {
    const mainTools: ToolCallItem[] = [];
    const subagents = new Map<string, ToolCallItem[]>();
    let failureCount = 0;
    for (const tool of toolCalls) {
      if (isToolFailed(tool)) failureCount += 1;
      const scope = subagentScope(tool.id);
      if (scope) {
        const tools = subagents.get(scope);
        if (tools) tools.push(tool);
        else subagents.set(scope, [tool]);
      } else mainTools.push(tool);
    }
    return { mainTools, subagents, failureCount };
  }, [toolCalls]);
  const summary = `${mainTools.length} main action${mainTools.length === 1 ? '' : 's'}${subagents.size ? ` · ${subagents.size} subagent${subagents.size === 1 ? '' : 's'}` : ''}`;

  const hasRunning = isStreaming || toolCalls.some((t) => t.status === 'running' || t.status === 'pending');
  const hasCancelled = !hasRunning && toolCalls.some((t) => t.status === 'cancelled');

  // Manual choices apply within a phase, not across live/completed transitions.
  useEffect(() => {
    setManualCollapsed(null);
  }, [hasRunning]);

  const isCollapsed = manualCollapsed ?? !hasRunning;

  const toggleDetails = (id: string, isOpen: boolean) => {
    setExpandedDetails((prev) => ({
      ...prev,
      [id]: !isOpen,
    }));
  };

  const renderTool = (tool: ToolCallItem) => {
    let info = displayInfoCache.current.get(tool);
    if (!info) {
      info = getToolDisplayInfo(tool);
      displayInfoCache.current.set(tool, info);
    }
    const failed = isToolFailed(tool);
    const cancelled = tool.status === 'cancelled';
    const isDetailsOpen = expandedDetails[tool.id] ?? (tool.status === 'running' || failed);
    const detailsId = `${traceId}-${tool.id}-details`;
    const duration = formatDuration(tool.durationMs);
    const selectsArtifact = !info.isTerminal && Boolean(tool.artifactId && onSelectArtifact);
    const isToolRunning = tool.status === 'running' || tool.status === 'pending';

    return (
      <div key={tool.id} className="group">
        <div className="flex items-center">
          <button
            type="button"
            aria-expanded={selectsArtifact ? undefined : isDetailsOpen}
            aria-controls={selectsArtifact ? undefined : detailsId}
            data-file-link={info.filePath ? 'true' : undefined}
            data-file-path={info.filePath}
            data-code-block={info.command ? 'true' : undefined}
            data-code-content={info.command ? safeEncodeURIComponent(info.command) : undefined}
            data-code-lang={info.command ? 'powershell' : undefined}
            onClick={() => {
              if (selectsArtifact && tool.artifactId && onSelectArtifact) {
                onSelectArtifact(tool.artifactId);
              } else {
                toggleDetails(tool.id, isDetailsOpen);
              }
            }}
            className="flex min-w-0 flex-1 items-center justify-between rounded-md px-2 py-1 text-left text-[11px] font-mono transition-colors duration-100 hover:transition-none cursor-pointer hover:bg-[var(--ui-row-hover)] focus-visible:outline focus-visible:outline-[var(--ui-accent)]"
          >
            <span className="flex items-center gap-2 min-w-0 flex-1 mr-3">
              {info.icon}
              <span className="truncate text-[var(--ui-text-secondary)]">{failed || cancelled ? tool.name : info.label}</span>
            </span>{' '}
            <span className="flex items-center gap-2 shrink-0">
              {isToolRunning && <ActiveToolDuration startedAt={tool.startedAt} />}

              {failed && (
                <span className="rounded bg-[var(--ui-diff-remove-bg)] px-1.5 py-0.5 text-[9px] font-medium text-[var(--ui-red)]">
                  Failed
                </span>
              )}

              {cancelled && (
                <span className="rounded bg-[var(--ui-control-fill)] px-1.5 py-0.5 text-[9px] font-medium text-[var(--conversation-scaffold-meta)]">
                  Cancelled
                </span>
              )}

              {!isToolRunning && duration && (
                <span className="text-[10px] text-[var(--conversation-scaffold-meta)]">{duration}</span>
              )}

              {!selectsArtifact && (
                <span className="text-[var(--conversation-scaffold-meta)] group-hover:text-[var(--ui-text-primary)]">
                  {isDetailsOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                </span>
              )}
            </span>
          </button>
          {selectsArtifact && (
            <button
              type="button"
              aria-label={`Details for ${tool.name}`}
              aria-expanded={isDetailsOpen}
              aria-controls={detailsId}
              onClick={() => toggleDetails(tool.id, isDetailsOpen)}
              className="shrink-0 rounded px-2 py-1 text-[var(--conversation-scaffold-meta)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)] focus-visible:outline focus-visible:outline-[var(--ui-accent)]"
            >
              {isDetailsOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            </button>
          )}
        </div>

        {isDetailsOpen && (
          <div id={detailsId} className="mx-1 mb-1.5 mt-0.5 rounded-lg bg-[var(--ui-widget-bg)] px-2.5 py-2">
            <div className="text-[10px] text-[var(--conversation-scaffold-meta)]">Arguments</div>
            <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words select-text text-[11px] text-[var(--ui-text-secondary)]" data-selectable-text="true">
              {tool.argumentsRaw || JSON.stringify(tool.args, null, 2)}
            </pre>
            {info.isTerminal ? (
              <TerminalOutput
                command={info.command}
                output={info.output}
                exitCode={tool.exitCode}
                isRunning={tool.status === 'running'}
              />
            ) : (
              <>
                {tool.name === 'search_web' && (() => {
                  const links = parseSearchLinks(tool.result);
                  if (links.length === 0) return null;
                  return (
                    <div className="mt-2 mb-2">
                      <div className="mb-1.5 flex items-center gap-1 text-[10px] font-semibold text-[var(--conversation-scaffold-meta)]">
                        <Globe size={11} /> Discovered Web Sources ({links.length})
                      </div>
                      <div className="flex flex-wrap gap-1.5">
                        {links.map((link, idx) => {
                          let host = link.url;
                          try { host = new URL(link.url).hostname.replace(/^www\./, ''); } catch {}
                          return (
                            <a
                              key={idx}
                              href={link.url}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="inline-flex items-center gap-1 rounded border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-elevated)] px-2 py-1 text-[10px] text-[var(--ui-accent)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:border-[var(--ui-accent-border)]"
                              title={`${link.title}\n${link.url}`}
                            >
                              <ExternalLink size={10} className="shrink-0 text-[var(--conversation-scaffold-meta)]" />
                              <span className="font-semibold text-[var(--ui-text-secondary)]">{host}:</span>
                              <span className="max-w-[160px] truncate">{link.title}</span>
                            </a>
                          );
                        })}
                      </div>
                    </div>
                  );
                })()}
                <div className="mt-2 text-[10px] text-[var(--conversation-scaffold-meta)]">Result</div>
                <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words select-text text-[11px] text-[var(--ui-text-secondary)]" data-selectable-text="true">
                  {tool.result || tool.terminalOutput || (info.isRunning ? 'Waiting for result...' : '(no result)')}
                </pre>
              </>
            )}
          </div>
        )}
      </div>
    );
  };

  if (!toolCalls || toolCalls.length === 0) return null;

  return (
    <div
      className="my-2 w-full text-xs text-[var(--conversation-scaffold-text)]"
      data-testid="execution-trace"
    >
      {/* Summary Accordion Bar */}
      <button
        type="button"
        onClick={() => setManualCollapsed(!isCollapsed)}
        className="flex w-full items-center justify-between rounded-md px-1.5 py-1 text-left text-[var(--conversation-scaffold-text)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)] select-none"
        aria-expanded={!isCollapsed}
      >
        <div className="flex items-center gap-2 min-w-0 flex-1 mr-2">
          {hasRunning ? (
            <Loader2 size={13} className="text-[var(--ui-orange)] animate-spin shrink-0" />
          ) : (
            <>
              <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--ui-tool-dot)]" aria-hidden="true" />
              {isCollapsed ? (
                <ChevronRight size={12} className="text-[var(--conversation-scaffold-meta)] shrink-0" />
              ) : (
                <ChevronDown size={12} className="text-[var(--conversation-scaffold-meta)] shrink-0" />
              )}
            </>
          )}
          <span className="truncate text-[11.5px] font-medium text-[var(--conversation-scaffold-text)]">
            {summary}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-2">
        {failureCount > 0 && <span className="text-[10px] font-mono text-[#f85149]">{failureCount} failed</span>}
        {hasRunning ? (
          <span className="flex items-center gap-1.5 shrink-0 text-[10px] text-[var(--ui-orange)] font-mono animate-pulse">
            <span className="w-1.5 h-1.5 rounded-full bg-[var(--ui-orange)]" />
            in progress
          </span>
        ) : hasCancelled ? (
          <span className="flex items-center gap-1 shrink-0 text-[10px] text-[var(--conversation-scaffold-meta)] font-mono">
            <Ban size={10} className="text-[var(--conversation-scaffold-meta)]" />
            cancelled
          </span>
        ) : null}
        </div>
      </button>

      {/* Expanded Detailed Status Rows */}
      {!isCollapsed && (
        <div className="mt-1 ml-2 max-h-80 overflow-y-auto overscroll-contain border-l border-[var(--ui-stroke-tertiary)] pl-2 space-y-0.5">
          {mainTools.map(renderTool)}
          {Array.from(subagents, ([scope, tools]) => {
            const open = expandedSubagents[scope] ?? false;
            const failed = tools.filter(isToolFailed).length;
            const running = tools.some((tool) => tool.status === 'running' || tool.status === 'pending');
            const groupId = `${traceId}-${scope}-tools`;
            return <div key={scope}>
              <button type="button" aria-expanded={open} aria-controls={groupId}
                onClick={() => setExpandedSubagents((previous) => ({ ...previous, [scope]: !open }))}
                className="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-[11px] text-[var(--conversation-scaffold-text)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] hover:text-[var(--ui-text-primary)] focus-visible:outline focus-visible:outline-[var(--ui-accent)]">
                {open ? <ChevronDown size={12} className="shrink-0" /> : <ChevronRight size={12} className="shrink-0" />}
                <span className="min-w-0 flex-1 truncate">Subagent {scope} · {tools.length} action{tools.length === 1 ? '' : 's'}</span>{' '}
                {failed > 0 && <span className="shrink-0 text-[var(--ui-red)]">{failed} failed</span>}
                {running && <Loader2 size={12} className="shrink-0 animate-spin text-[var(--ui-orange)]" aria-label="in progress" />}
              </button>
              {open && <div id={groupId} className="ml-3 border-l border-[var(--ui-stroke-tertiary)] pl-1">{tools.map(renderTool)}</div>}
            </div>;
          })}

        </div>
      )}
    </div>
  );
};
