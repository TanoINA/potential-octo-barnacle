import React, { useState, useMemo, useEffect, useId, useRef } from 'react';
import {
  Ban,
  BookOpen,
  Bot,
  ChevronDown,
  ChevronRight,
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
    <span className="flex items-center gap-1.5 rounded bg-[#f0883e]/10 px-1.5 py-0.5 text-[10px] font-mono font-medium text-[#f0883e] border border-[#f0883e]/30 animate-pulse">
      <Loader2 size={10} className="animate-spin text-[#f0883e]" />
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
            className="flex min-w-0 flex-1 items-center justify-between px-3.5 py-1.5 text-left text-[11px] font-mono transition-colors cursor-pointer hover:bg-[#161b22]/60 focus-visible:outline focus-visible:outline-[#58a6ff]"
          >
            <span className="flex items-center gap-2 min-w-0 flex-1 mr-3">
              {info.icon}
              <span className="truncate text-[#c9d1d9]">{failed || cancelled ? tool.name : info.label}</span>
            </span>{' '}
            <span className="flex items-center gap-2 shrink-0">
              {isToolRunning && <ActiveToolDuration startedAt={tool.startedAt} />}

              {failed && (
                <span className="rounded bg-[#da3633]/20 px-1.5 py-0.5 text-[9px] font-medium text-[#f85149]">
                  Failed
                </span>
              )}

              {cancelled && (
                <span className="rounded bg-[#30363d]/50 px-1.5 py-0.5 text-[9px] font-medium text-[#8b949e]">
                  Cancelled
                </span>
              )}

              {!isToolRunning && duration && (
                <span className="text-[10px] text-[#6e7681]">{duration}</span>
              )}

              {!selectsArtifact && (
                <span className="text-[#6e7681] group-hover:text-[#c9d1d9]">
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
              className="shrink-0 px-3 py-1.5 text-[#8b949e] hover:text-[#c9d1d9] focus-visible:outline focus-visible:outline-[#58a6ff]"
            >
              {isDetailsOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
            </button>
          )}
        </div>

        {isDetailsOpen && (
          <div id={detailsId} className="px-3 pb-2 pt-0.5">
            <div className="text-[10px] text-[#8b949e]">Arguments</div>
            <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words select-text text-[11px] text-[#c9d1d9]" data-selectable-text="true">
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
                <div className="mt-2 text-[10px] text-[#8b949e]">Result</div>
                <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words select-text text-[11px] text-[#c9d1d9]" data-selectable-text="true">
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
      className="my-2.5 w-full rounded-lg border border-[#30363d]/70 bg-[#0d1117]/60 overflow-hidden text-xs"
      data-testid="execution-trace"
    >
      {/* Summary Accordion Bar */}
      <button
        type="button"
        onClick={() => setManualCollapsed(!isCollapsed)}
        className="flex w-full items-center justify-between px-3 py-2 text-left text-[#8b949e] hover:text-[#c9d1d9] hover:bg-[#161b22]/70 transition-colors select-none"
        aria-expanded={!isCollapsed}
      >
        <div className="flex items-center gap-2 min-w-0 flex-1 mr-2">
          {hasRunning ? (
            <Loader2 size={13} className="text-[#f0883e] animate-spin shrink-0" />
          ) : isCollapsed ? (
            <ChevronRight size={13} className="text-[#6e7681] shrink-0" />
          ) : (
            <ChevronDown size={13} className="text-[#6e7681] shrink-0" />
          )}
          <span className="truncate text-[11px] font-mono text-[#8b949e]">
            {summary}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-2">
        {failureCount > 0 && <span className="text-[10px] font-mono text-[#f85149]">{failureCount} failed</span>}
        {hasRunning ? (
          <span className="flex items-center gap-1.5 shrink-0 text-[10px] text-[#f0883e] font-mono animate-pulse">
            <span className="w-1.5 h-1.5 rounded-full bg-[#f0883e]" />
            in progress
          </span>
        ) : hasCancelled ? (
          <span className="flex items-center gap-1 shrink-0 text-[10px] text-[#8b949e] font-mono">
            <Ban size={10} className="text-[#8b949e]" />
            cancelled
          </span>
        ) : null}
        </div>
      </button>

      {/* Expanded Detailed Status Rows */}
      {!isCollapsed && (
        <div className="max-h-80 overflow-y-auto overscroll-contain border-t border-[#21262d] divide-y divide-[#21262d]/50 bg-[#0d1117]">
          {mainTools.map(renderTool)}
          {Array.from(subagents, ([scope, tools]) => {
            const open = expandedSubagents[scope] ?? false;
            const failed = tools.filter(isToolFailed).length;
            const running = tools.some((tool) => tool.status === 'running' || tool.status === 'pending');
            const groupId = `${traceId}-${scope}-tools`;
            return <div key={scope}>
              <button type="button" aria-expanded={open} aria-controls={groupId}
                onClick={() => setExpandedSubagents((previous) => ({ ...previous, [scope]: !open }))}
                className="flex w-full items-center gap-2 px-3 py-2 text-left text-[11px] text-[#8b949e] hover:bg-[#161b22]/60 focus-visible:outline focus-visible:outline-[#58a6ff]">
                {open ? <ChevronDown size={12} className="shrink-0" /> : <ChevronRight size={12} className="shrink-0" />}
                <span className="min-w-0 flex-1 truncate">Subagent {scope} · {tools.length} action{tools.length === 1 ? '' : 's'}</span>{' '}
                {failed > 0 && <span className="shrink-0 text-[#f85149]">{failed} failed</span>}
                {running && <Loader2 size={12} className="shrink-0 animate-spin text-[#f0883e]" aria-label="in progress" />}
              </button>
              {open && <div id={groupId} className="border-l border-[#30363d] ml-3">{tools.map(renderTool)}</div>}
            </div>;
          })}

        </div>
      )}
    </div>
  );
};
