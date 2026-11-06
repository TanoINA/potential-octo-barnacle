import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Check, ChevronDown, ChevronUp, ExternalLink, FileCode2, GitBranch, GitCommit, GitCompare, GitPullRequest, Loader2 } from 'lucide-react';
import type { FileDiffStat, ToolCallItem } from '../types/agent';

interface FilesChangedCardProps {
  toolCalls: ToolCallItem[];
  onSelectArtifact?: (id: string) => void;
}

interface GitStatusResult {
  isRepo: boolean;
  branch: string;
  hasChanges: boolean;
  summary: string;
}

export const FilesChangedCard: React.FC<FilesChangedCardProps> = ({
  toolCalls,
  onSelectArtifact,
}) => {
  // Aggregate all files changed across tools
  const { files, totalAdditions, totalDeletions } = React.useMemo(() => {
    const fileMap: Record<string, { stat: FileDiffStat; artifactId?: string }> = {};
    let totalAdd = 0;
    let totalDel = 0;

    toolCalls.forEach((tool) => {
      if (tool.filesChanged && tool.filesChanged.length > 0) {
        tool.filesChanged.forEach((fc) => {
          const key = fc.path;
          if (!fileMap[key]) {
            fileMap[key] = { stat: { ...fc }, artifactId: tool.artifactId };
          } else {
            fileMap[key].stat.additions += fc.additions;
            fileMap[key].stat.deletions += fc.deletions;
            if (tool.artifactId) fileMap[key].artifactId = tool.artifactId;
          }
          totalAdd += fc.additions;
          totalDel += fc.deletions;
        });
      }
    });

    return {
      files: Object.values(fileMap),
      totalAdditions: totalAdd,
      totalDeletions: totalDel,
    };
  }, [toolCalls]);

  const [gitStatus, setGitStatus] = useState<GitStatusResult | null>(null);
  const [showCommitBar, setShowCommitBar] = useState(false);
  const [commitMessage, setCommitMessage] = useState('');
  const [isCommitting, setIsCommitting] = useState(false);
  const [commitResult, setCommitResult] = useState<{ success: boolean; message: string } | null>(null);

  // Auto-generate a suggested conventional commit message
  useEffect(() => {
    if (files.length === 1) {
      const name = files[0].stat.path.replace(/.*[/\\]/, '');
      setCommitMessage(`feat: update ${name}`);
    } else if (files.length > 1) {
      const names = files.slice(0, 2).map((f) => f.stat.path.replace(/.*[/\\]/, '')).join(', ');
      setCommitMessage(`feat: update ${names}${files.length > 2 ? ' and more' : ''}`);
    }
  }, [files]);

  // Check git status in workspace
  useEffect(() => {
    let active = true;
    invoke<GitStatusResult>('git_status')
      .then((status) => {
        if (active) setGitStatus(status);
      })
      .catch(() => {
        if (active) setGitStatus(null);
      });
    return () => {
      active = false;
    };
  }, []);

  const handleCommit = async (push: boolean) => {
    if (!commitMessage.trim()) return;
    setIsCommitting(true);
    setCommitResult(null);
    try {
      const commitOut = await invoke<string>('git_commit', { message: commitMessage.trim() });
      if (push) {
        const pushOut = await invoke<string>('git_push');
        setCommitResult({ success: true, message: `Committed & Pushed: ${pushOut || commitOut}` });
      } else {
        setCommitResult({ success: true, message: `Committed: ${commitOut}` });
      }
      setShowCommitBar(false);
    } catch (err) {
      setCommitResult({ success: false, message: String(err) });
    } finally {
      setIsCommitting(false);
    }
  };

  if (files.length === 0) return null;

  return (
    <div className="mt-3 w-full rounded-xl border border-[#30363d] bg-[#0d1117] overflow-hidden shadow-sm">
      {/* Header Bar */}
      <div className="flex items-center justify-between border-b border-[#21262d] bg-[#161b22] px-3.5 py-2 text-xs">
        <div className="flex items-center gap-2">
          <GitCompare size={14} className="text-[#58a6ff]" />
          <span className="font-medium text-[#e6edf3]">
            {files.length} file{files.length > 1 ? 's' : ''} changed
          </span>
          {gitStatus?.isRepo && (
            <span className="inline-flex items-center gap-1 rounded bg-[#21262d] px-2 py-0.5 font-mono text-[10px] text-[#8b949e]">
              <GitBranch size={10} className="text-[#58a6ff]" />
              {gitStatus.branch}
            </span>
          )}
        </div>

        <div className="flex items-center gap-2">
          <div className="flex items-center gap-1.5 font-mono text-[11px]">
            {totalAdditions > 0 && (
              <span className="text-[#3fb950] font-medium">+{totalAdditions}</span>
            )}
            {totalDeletions > 0 && (
              <span className="text-[#f85149] font-medium">-{totalDeletions}</span>
            )}
          </div>
          {gitStatus?.isRepo && (
            <button
              type="button"
              onClick={() => setShowCommitBar((prev) => !prev)}
              className="flex items-center gap-1 rounded border border-[#30363d] bg-[#21262d] px-2 py-0.5 text-[10px] font-medium text-[#e6edf3] hover:border-[#58a6ff] hover:text-[#58a6ff] transition-colors"
              title="Toggle Git Commit Bar"
            >
              <GitCommit size={11} className="text-[#58a6ff]" />
              <span>Commit</span>
              {showCommitBar ? <ChevronUp size={11} /> : <ChevronDown size={11} />}
            </button>
          )}
        </div>
      </div>

      {/* File List */}
      <div className="divide-y divide-[#21262d]/60 bg-[#0d1117] p-1">
        {files.map(({ stat, artifactId }) => {
          const filename = stat.path.replace(/.*[/\\]/, '');
          const dir = stat.path.includes('/') || stat.path.includes('\\')
            ? stat.path.replace(/[/\\][^/\\]+$/, '')
            : '';

          return (
            <div
              key={stat.path}
              className="flex items-center justify-between px-3 py-2 text-xs hover:bg-[#161b22]/40 rounded-lg transition-colors group"
            >
              <div className="flex items-center gap-2 min-w-0 flex-1 mr-3">
                <FileCode2 size={14} className="text-[#8b949e] shrink-0" />
                <span className="truncate font-mono text-[11px] text-[#c9d1d9]" title={stat.path}>
                  {dir ? <span className="text-[#6e7681] mr-1">{dir}/</span> : null}
                  <span className="text-white font-medium">{filename}</span>
                </span>
              </div>

              <div className="flex items-center gap-2 shrink-0">
                {/* Diff stats badge */}
                <div className="flex items-center gap-1 font-mono text-[10px]">
                  {stat.additions > 0 && (
                    <span className="px-1.5 py-0.2 rounded bg-[#238636]/20 text-[#3fb950] font-medium border border-[#238636]/30">
                      +{stat.additions}
                    </span>
                  )}
                  {stat.deletions > 0 && (
                    <span className="px-1.5 py-0.2 rounded bg-[#da3633]/20 text-[#f85149] font-medium border border-[#da3633]/30">
                      -{stat.deletions}
                    </span>
                  )}
                </div>

                {/* Review Button */}
                {artifactId && onSelectArtifact && (
                  <button
                    type="button"
                    onClick={() => onSelectArtifact(artifactId)}
                    className="flex items-center gap-1 rounded border border-[#30363d] bg-[#21262d] px-2 py-0.5 text-[10px] text-[#58a6ff] hover:bg-[#30363d] hover:text-white transition-colors"
                    title="Review unified diff"
                  >
                    <span>Review</span>
                    <ExternalLink size={10} />
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>

      {/* One-Click Git Commit & Push Bar */}
      {showCommitBar && gitStatus?.isRepo && (
        <div className="border-t border-[#30363d] bg-[#161b22] p-3 space-y-2">
          <div className="flex items-center gap-2">
            <input
              type="text"
              value={commitMessage}
              onChange={(e) => setCommitMessage(e.target.value)}
              placeholder="Commit message (e.g. feat: add terminal support)..."
              disabled={isCommitting}
              className="flex-1 rounded border border-[#30363d] bg-[#0d1117] px-2.5 py-1.5 font-mono text-xs text-[#e6edf3] placeholder:text-[#6e7681] outline-none focus:border-[#58a6ff]"
            />
            <button
              type="button"
              onClick={() => handleCommit(false)}
              disabled={isCommitting || !commitMessage.trim()}
              className="flex items-center gap-1.5 rounded bg-[#238636] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#2ea043] disabled:opacity-50 transition-colors shrink-0"
              title="Stage all changes and commit"
            >
              {isCommitting ? <Loader2 size={12} className="animate-spin" /> : <GitCommit size={12} />}
              <span>Commit</span>
            </button>
            <button
              type="button"
              onClick={() => handleCommit(true)}
              disabled={isCommitting || !commitMessage.trim()}
              className="flex items-center gap-1.5 rounded bg-[#1f6feb] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#388bfd] disabled:opacity-50 transition-colors shrink-0"
              title="Commit and push to remote origin"
            >
              {isCommitting ? <Loader2 size={12} className="animate-spin" /> : <GitPullRequest size={12} />}
              <span>Commit & Push</span>
            </button>
          </div>
        </div>
      )}

      {/* Commit Result Toast/Alert */}
      {commitResult && (
        <div
          className={`flex items-center justify-between border-t px-3 py-1.5 text-xs ${
            commitResult.success
              ? 'border-[#238636]/40 bg-[#238636]/10 text-[#3fb950]'
              : 'border-[#da3633]/40 bg-[#da3633]/10 text-[#f85149]'
          }`}
        >
          <div className="flex items-center gap-2 truncate">
            {commitResult.success ? <Check size={13} /> : null}
            <span className="truncate">{commitResult.message}</span>
          </div>
          <button
            type="button"
            onClick={() => setCommitResult(null)}
            className="text-[11px] underline opacity-80 hover:opacity-100 ml-2 shrink-0"
          >
            Dismiss
          </button>
        </div>
      )}
    </div>
  );
};
