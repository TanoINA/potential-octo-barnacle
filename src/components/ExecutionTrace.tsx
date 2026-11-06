import React, { useState, useMemo } from 'react';
import {
  BookOpen,
  ChevronDown,
  ChevronRight,
  FileCode2,
  FileText,
  Globe,
  Loader2,
  Search,
  Settings,
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

function getToolDisplayInfo(tool: ToolCallItem): {
  icon: React.ReactNode;
  label: string;
  isTerminal: boolean;
  command?: string;
  output?: string;
} {
  const name = tool.name;
  const args = tool.args || {};

  if (name === 'use_skill' || (name === 'read_file' && String(args.path || args.filePath || '').endsWith('SKILL.md'))) {
    const skillName = typeof args.name === 'string' ? args.name : String(args.path || args.filePath || '').replace(/.*[/\\]([^/\\]+)[/\\]SKILL\.md$/, '$1');
    return {
      icon: <BookOpen size={13} className="text-[#f0883e] shrink-0" />,
      label: `Loaded skill: ${skillName}`,
      isTerminal: false,
    };
  }

  if (name === 'run_command') {
    const command = typeof args.command === 'string' ? args.command : '';
    const displayCmd = command.length > 70 ? `${command.slice(0, 67)}...` : command;
    return {
      icon: <Settings size={13} className="text-[#8b949e] shrink-0" />,
      label: `Ran "${displayCmd}"`,
      isTerminal: true,
      command,
      output: tool.terminalOutput || tool.result || '',
    };
  }

  if (name === 'read_file') {
    const path = String(args.path || args.filePath || 'file');
    const filename = path.replace(/.*[/\\]/, '');
    return {
      icon: <FileText size={13} className="text-[#8b949e] shrink-0" />,
      label: `Read ${filename}`,
      isTerminal: false,
    };
  }

  if (name === 'search_files') {
    const pattern = typeof args.pattern === 'string' ? args.pattern : '';
    return {
      icon: <Search size={13} className="text-[#8b949e] shrink-0" />,
      label: `Searched files "${pattern}"`,
      isTerminal: false,
    };
  }

  if (name === 'list_directory') {
    const path = typeof args.path === 'string' ? args.path : '.';
    return {
      icon: <FileCode2 size={13} className="text-[#8b949e] shrink-0" />,
      label: `Explored directory "${path}"`,
      isTerminal: false,
    };
  }

  if (name === 'edit_file' || name === 'write_file') {
    const path = String(args.path || 'file');
    const filename = path.replace(/.*[/\\]/, '');
    return {
      icon: <FileCode2 size={13} className="text-[#58a6ff] shrink-0" />,
      label: `${name === 'edit_file' ? 'Edited' : 'Wrote'} ${filename}`,
      isTerminal: false,
    };
  }

  if (name === 'fetch_web_page') {
    const url = typeof args.url === 'string' ? args.url : '';
    let host = url;
    try {
      host = new URL(url).hostname;
    } catch {
      // ignore
    }
    return {
      icon: <Globe size={13} className="text-[#58a6ff] shrink-0" />,
      label: `Fetched ${host}`,
      isTerminal: false,
    };
  }

  if (name === 'learn_skill') {
    const skillName = typeof args.name === 'string' ? args.name : 'skill';
    return {
      icon: <BookOpen size={13} className="text-[#f0883e] shrink-0" />,
      label: `Learned skill: ${skillName}`,
      isTerminal: false,
    };
  }

  return {
    icon: <Settings size={13} className="text-[#8b949e] shrink-0" />,
    label: `${name}`,
    isTerminal: false,
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

    return parts.length > 0 ? parts.join(', ') : `${toolCalls.length} step${toolCalls.length > 1 ? 's' : ''} executed`;
  }, [toolCalls]);

  const hasRunning = isStreaming || toolCalls.some((t) => t.status === 'running' || t.status === 'pending');
  const hasError = toolCalls.some((t) => t.status === 'error' || (t.exitCode !== undefined && t.exitCode !== 0));

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
    <div className="my-2.5 w-full rounded-lg border border-[#30363d]/70 bg-[#0d1117]/60 overflow-hidden text-xs">
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

        {hasRunning && (
          <span className="flex items-center gap-1.5 shrink-0 text-[10px] text-[#f0883e] font-mono animate-pulse">
            <span className="w-1.5 h-1.5 rounded-full bg-[#f0883e]" />
            in progress
          </span>
        )}
      </button>

      {/* Expanded Detailed Status Rows */}
      {!isCollapsed && (
        <div className="border-t border-[#21262d] divide-y divide-[#21262d]/50 bg-[#0d1117]">
          {toolCalls.map((tool) => {
            const info = getToolDisplayInfo(tool);
            const isTerminalOpen = expandedTerminals[tool.id] !== undefined ? expandedTerminals[tool.id] : tool.status === 'running' || tool.status === 'error';
            const duration = formatDuration(tool.durationMs);
            const isClickable = info.isTerminal || Boolean(tool.artifactId && onSelectArtifact);

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
                    {tool.status === 'running' && (
                      <span className="flex items-center gap-1 text-[10px] text-[#f0883e]">
                        <Loader2 size={10} className="animate-spin" />
                      </span>
                    )}

                    {duration && (
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
