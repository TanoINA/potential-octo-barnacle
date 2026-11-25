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
  docked?: boolean;
}

export function ActivityTray({
  subagents = [], tasks = [], activeSkills = [], onSelectArtifact, onKillTask, onRefreshTasks, docked = false,
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
    <div className={docked
      ? 'w-full min-w-0 border-b border-[var(--ui-stroke-quaternary)] bg-transparent'
      : 'mx-auto mb-2 w-full min-w-0 max-w-3xl rounded-lg border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-elevated)]'}
    >
      <button
        type="button"
        aria-expanded={expanded}
        aria-controls={panelId}
        onClick={() => setExpanded((value) => !value)}
        className={`flex h-8 w-full min-w-0 flex-nowrap items-center gap-2 overflow-hidden whitespace-nowrap px-3 text-left text-[11px] text-[var(--ui-text-tertiary)] transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ui-accent)] ${docked ? '' : 'rounded-lg'}`}
      >
        <Activity size={13} aria-hidden="true" className="shrink-0 text-[var(--ui-accent)]" />
        <span className="shrink-0 font-semibold text-[var(--ui-text-primary)]">Activity</span>
        <span className="min-w-0 flex-1 truncate">{summary}</span>
        {runningCount > 0 && <span className="shrink-0 text-[var(--ui-orange)]">{runningCount} running</span>}
        {failedCount > 0 && <span className="shrink-0 text-[var(--ui-red)]">{failedCount} failed</span>}
        <ChevronDown size={13} aria-hidden="true" className={`shrink-0 ${expanded ? 'rotate-180' : ''}`} />
      </button>
      {expanded && (
        <div
          id={panelId}
          role="region"
          aria-label="Activity details"
          tabIndex={0}
          className="max-h-[min(320px,35vh)] overflow-x-hidden overflow-y-auto overscroll-contain border-t border-[var(--ui-stroke-quaternary)] p-2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ui-accent)]"
        >
          <SubagentTreeCard subagents={subagents} onSelectArtifact={onSelectArtifact} />
          <BackgroundTasksCard tasks={tasks} onKillTask={onKillTask} onRefreshTasks={onRefreshTasks} />
          {activeSkills.length > 0 && (
            <section className="rounded-lg border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-chrome)] p-2.5">
              <h3 className="mb-2 text-xs font-semibold text-[var(--ui-text-primary)]">Active Skills ({activeSkills.length})</h3>
              <ul aria-label="Active skills" className="space-y-2">
                {activeSkills.map((skill) => (
                  <li key={skill.path || skill.name} className="min-w-0 break-words text-[11px]">
                    <span className="font-medium text-[var(--ui-green)]">{skill.name}</span>
                    <p className="mt-0.5 text-[var(--ui-text-tertiary)]">{skill.description}</p>
                    <p className="mt-0.5 break-all font-mono text-[10px] text-[var(--ui-text-quaternary)]">{skill.path}</p>
                    <span className="text-[10px] text-[var(--ui-text-quaternary)]">{skill.source}</span>
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
