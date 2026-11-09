import { useId, useState } from 'react';
import { Activity, ChevronDown } from 'lucide-react';
import type { BackgroundTask, SkillInfo, SubagentProgress } from '../types/agent';
import { BackgroundTasksCard } from './BackgroundTasksCard';
import { SubagentTreeCard } from './SubagentTreeCard';

interface ActivityTrayProps {
  subagents?: SubagentProgress[];
  tasks?: BackgroundTask[];
  activeSkills?: SkillInfo[];
  onSelectArtifact?: (artifactId: string) => void;
  onKillTask?: (taskId: string) => void;
  onRefreshTasks?: () => void;
}

export function ActivityTray({
  subagents = [], tasks = [], activeSkills = [], onSelectArtifact, onKillTask, onRefreshTasks,
}: ActivityTrayProps) {
  const [expanded, setExpanded] = useState(false);
  const panelId = useId();
  if (!subagents.length && !tasks.length && !activeSkills.length) return null;

  const runningCount = subagents.filter((item) => item.status === 'running').length
    + tasks.filter((item) => item.status === 'running').length;
  const failedCount = subagents.filter((item) => item.status === 'failed' || item.status === 'error').length
    + tasks.filter((item) => item.status === 'failed').length;
  const summary = `${subagents.length} subagents · ${tasks.length} tasks · ${activeSkills.length} skills`;

  return (
    <div className="mx-auto mb-2 w-full min-w-0 max-w-3xl rounded-lg border border-[#30363d] bg-[#161b22]">
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={panelId}
        onClick={() => setExpanded((value) => !value)}
        className="flex h-8 w-full min-w-0 flex-nowrap items-center gap-2 overflow-hidden whitespace-nowrap rounded-lg px-2.5 text-left text-[11px] text-[#8b949e] hover:bg-[#21262d] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#58a6ff]"
      >
        <Activity size={13} aria-hidden="true" className="shrink-0 text-[#58a6ff]" />
        <span className="shrink-0 font-semibold text-[#e6edf3]">Activity</span>
        <span className="min-w-0 flex-1 truncate">{summary}</span>
        {runningCount > 0 && <span className="shrink-0 text-[#f0883e]">{runningCount} running</span>}
        {failedCount > 0 && <span className="shrink-0 text-[#f85149]">{failedCount} failed</span>}
        <ChevronDown size={13} aria-hidden="true" className={`shrink-0 ${expanded ? 'rotate-180' : ''}`} />
      </button>
      {expanded && (
        <div
          id={panelId}
          role="region"
          aria-label="Activity details"
          tabIndex={0}
          className="max-h-[min(320px,35vh)] overflow-x-hidden overflow-y-auto overscroll-contain border-t border-[#30363d] p-2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#58a6ff]"
        >
          <SubagentTreeCard subagents={subagents} onSelectArtifact={onSelectArtifact} />
          <BackgroundTasksCard tasks={tasks} onKillTask={onKillTask} onRefreshTasks={onRefreshTasks} />
          {activeSkills.length > 0 && (
            <section className="rounded-lg border border-[#30363d] bg-[#0d1117] p-2.5">
              <h3 className="mb-2 text-xs font-semibold text-[#e6edf3]">Active Skills ({activeSkills.length})</h3>
              <ul aria-label="Active skills" className="space-y-2">
                {activeSkills.map((skill) => (
                  <li key={skill.path || skill.name} className="min-w-0 break-words text-[11px]">
                    <span className="font-medium text-[#3fb950]">{skill.name}</span>
                    <p className="mt-0.5 text-[#8b949e]">{skill.description}</p>
                    <p className="mt-0.5 break-all font-mono text-[10px] text-[#6e7681]">{skill.path}</p>
                    <span className="text-[10px] text-[#6e7681]">{skill.source}</span>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </div>
      )}
    </div>
  );
}
