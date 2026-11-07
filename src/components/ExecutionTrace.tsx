import React, { useState, useMemo, useEffect } from 'react';
import {
  Ban,
  BookOpen,
  Bot,
  ChevronDown,
  ChevronRight,
  FileCode2,
  FileText,
  Globe,
  Loader2,
  Search,
  Settings,
  Terminal,
} from 'lucide-react';
import type { ToolCallItem } from '../types/agent';
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
    const rawPath = extractArg(tool, 'path') || extractArg(tool, 'filePath') || 'file';
    const filename = rawPath.replace(/.*[/\\]/, '') || 'file';
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <FileText size={13} className="text-[#8b949e] shrink-0" />,
      label: isRunning ? `Reading ${filename}...` : `Read ${filename}`,
      isTerminal: false,
      isRunning,
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
    const path = extractArg(tool, 'path') || '.';
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <FileCode2 size={13} className="text-[#8b949e] shrink-0" />,
      label: isRunning ? `Exploring directory "${path}"...` : `Explored directory "${path}"`,
      isTerminal: false,
      isRunning,
    };
  }

  if (name === 'edit_file' || name === 'write_file') {
    const rawPath = extractArg(tool, 'path') || 'file';
    const filename = rawPath.replace(/.*[/\\]/, '') || 'file';
    const action = name === 'edit_file' ? 'Edit' : 'Write';
    return {
      icon: isRunning ? <Loader2 size={13} className="text-[#58a6ff] animate-spin shrink-0" /> : <FileCode2 size={13} className="text-[#58a6ff] shrink-0" />,
      label: isRunning ? `${action}ing ${filename}...` : `${name === 'edit_file' ? 'Edited' : 'Wrote'} ${filename}`,
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

export const ExecutionTrace: React.FC<ExecutionTraceProps> = ({
  toolCalls,
  isStreaming = false,
  onSelectArtifact,
}) => {
  const [manualCollapsed, setManualCollapsed] = useState<boolean | null>(null);
  const [expandedTerminals, setExpandedTerminals] = useState<Record<string, boolean>>({});

  // Summary counts
  const summary = useMemo(() => {
    let skills = 0;
    let files = 0;
    let commands = 0;
    let subagents = 0;
    let managedTasks = 0;
    const skillNames: string[] = [];

    toolCalls.forEach((tool) => {
      const name = tool.name;
      if (name === 'use_skill' || (name === 'read_file' && String(tool.args?.path || '').endsWith('SKILL.md'))) {
        skills += 1;
        const skillName = typeof tool.args?.name === 'string' ? tool.args.name : '';
        if (skillName && !skillNames.includes(skillName)) skillNames.push(skillName);
      } else if (name === 'read_file' || name === 'search_files' || name === 'list_directory') {
        files += 1;
      } else if (name === 'run_command') {
        commands += 1;
      } else if (name === 'spawn_subagent') {
        subagents += 1;
      } else if (name === 'manage_task') {
        managedTasks += 1;
      }
    });

    const parts: string[] = [];
    if (skillNames.length > 0) {
      parts.push(`Loaded skill: ${skillNames.slice(0, 2).join(', ')}${skillNames.length > 2 ? ` +${skillNames.length - 2}` : ''}`);
    } else if (skills > 0) {
      parts.push(`Loaded ${skills} skill${skills > 1 ? 's' : ''}`);
    }

    if (files > 0) parts.push(`explored ${files} file${files > 1 ? 's' : ''}`);
    if (commands > 0) parts.push(`ran ${commands} command${commands > 1 ? 's' : ''}`);
    if (subagents > 0) parts.push(`spawned ${subagents} subagent${subagents > 1 ? 's' : ''}`);
    if (managedTasks > 0) parts.push(`managed ${managedTasks} background task${managedTasks > 1 ? 's' : ''}`);

    return parts.length > 0 ? parts.join(', ') : `${toolCalls.length} step${toolCalls.length > 1 ? 's' : ''} executed`;
  }, [toolCalls]);

  const hasRunning = isStreaming || toolCalls.some((t) => t.status === 'running' || t.status === 'pending');
  const hasCancelled = !hasRunning && toolCalls.some((t) => t.status === 'cancelled');
  const hasError = toolCalls.some((t) => t.status === 'error' || (t.exitCode !== undefined && t.exitCode !== 0));

  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!hasRunning) return;
    const interval = setInterval(() => {
      setNow(Date.now());
    }, 250);
    return () => clearInterval(interval);
  }, [hasRunning]);

  // Default: open while running or if error; collapsed when completed unless user explicitly toggled
  const isCollapsed = manualCollapsed !== null ? manualCollapsed : hasRunning || hasError ? false : true;

  const toggleTerminal = (id: string) => {
    setExpandedTerminals((prev) => ({
      ...prev,
      [id]: !prev[id],
    }));
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
      </button>

      {/* Expanded Detailed Status Rows */}
      {!isCollapsed && (
        <div className="border-t border-[#21262d] divide-y divide-[#21262d]/50 bg-[#0d1117]">
          {toolCalls.map((tool) => {
            const info = getToolDisplayInfo(tool);
            const isTerminalOpen = expandedTerminals[tool.id] !== undefined ? expandedTerminals[tool.id] : tool.status === 'running' || tool.status === 'error';
            const duration = formatDuration(tool.durationMs);
            const isClickable = info.isTerminal || Boolean(tool.artifactId && onSelectArtifact);
            const isToolRunning = tool.status === 'running' || tool.status === 'pending';
            const liveElapsedSec = isToolRunning
              ? Math.max(1, Math.floor((now - (tool.startedAt || now)) / 1000))
              : null;

            return (
              <div key={tool.id} className="group">
                <div
                  onClick={() => {
                    if (info.isTerminal) {
                      toggleTerminal(tool.id);
                    } else if (tool.artifactId && onSelectArtifact) {
                      onSelectArtifact(tool.artifactId);
                    }
                  }}
                  className={`flex items-center justify-between px-3.5 py-1.5 text-[11px] font-mono transition-colors ${isClickable ? 'cursor-pointer hover:bg-[#161b22]/60' : ''}`}
                >
                  <div className="flex items-center gap-2 min-w-0 flex-1 mr-3">
                    {info.icon}
                    <span className="truncate text-[#c9d1d9]">{info.label}</span>
                  </div>

                  <div className="flex items-center gap-2 shrink-0">
                    {isToolRunning && (
                      <span className="flex items-center gap-1.5 rounded bg-[#f0883e]/10 px-1.5 py-0.5 text-[10px] font-mono font-medium text-[#f0883e] border border-[#f0883e]/30 animate-pulse">
                        <Loader2 size={10} className="animate-spin text-[#f0883e]" />
                        <span>{liveElapsedSec}s</span>
                      </span>
                    )}

                    {tool.status === 'cancelled' && (
                      <span className="rounded bg-[#30363d]/50 px-1.5 py-0.5 text-[9px] font-medium text-[#8b949e]">
                        cancelled
                      </span>
                    )}

                    {!isToolRunning && duration && (
                      <span className="text-[10px] text-[#6e7681]">{duration}</span>
                    )}

                    {info.isTerminal && (
                      <span className="text-[#6e7681] group-hover:text-[#c9d1d9]">
                        {isTerminalOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                      </span>
                    )}
                  </div>
                </div>

                {/* Inline Terminal Viewer for run_command */}
                {info.isTerminal && isTerminalOpen && (
                  <div className="px-3 pb-2 pt-0.5">
                    <TerminalOutput
                      command={info.command}
                      output={info.output}
                      exitCode={tool.exitCode}
                      isRunning={tool.status === 'running'}
                    />
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};
