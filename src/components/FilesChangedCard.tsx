import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Check, ChevronDown, ChevronUp, ExternalLink, FileCode2, GitBranch, GitCommit, GitCompare, GitPullRequest, Loader2, Undo2 } from 'lucide-react';
import type { FileDiffStat, ToolCallItem } from '../types/agent';

const UNSCOPED_RECEIPT_REASON = 'Review-only: this receipt has no verified workspace origin. Use the workspace Git controls for current changes.';

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
    const fileMap = new Map<string, { stat: FileDiffStat; artifactId?: string }>();
    let totalAdd = 0;
    let totalDel = 0;

    toolCalls.forEach((tool) => {
      if (tool.filesChanged && tool.filesChanged.length > 0) {
        tool.filesChanged.forEach((fc) => {
          const key = fc.path;
          const existing = fileMap.get(key);
          if (!existing) {
            fileMap.set(key, { stat: { ...fc }, artifactId: tool.artifactId });
          } else {
            existing.stat.additions += fc.additions;
            existing.stat.deletions += fc.deletions;
            if (tool.artifactId) existing.artifactId = tool.artifactId;
          }
          totalAdd += fc.additions;
          totalDel += fc.deletions;
        });
      }
    });

    return {
      files: Array.from(fileMap.values()),
      totalAdditions: totalAdd,
      totalDeletions: totalDel,
    };
  }, [toolCalls]);

  const [gitStatus, setGitStatus] = useState<GitStatusResult | null>(null);
  const [showCommitBar, setShowCommitBar] = useState(false);
  const [commitMessage, setCommitMessage] = useState('');
  const [isCommitting, setIsCommitting] = useState(false);
  const [commitResult, setCommitResult] = useState<{ success: boolean; message: string } | null>(null);
  const [fileActions, setFileActions] = useState<Record<string, 'reverted' | 'accepted' | 'loading'>>({});
  const [isMinimized, setIsMinimized] = useState(false);

  // Persisted tool receipts have no workspace/revision identity. Never infer it
  // from the currently selected workspace (which may now be another repo).
  const canMutate = false;
  const handleRevertFile = async (filePath: string) => {
    if (!canMutate) return;
    setFileActions((prev) => ({ ...prev, [filePath]: 'loading' }));
    try {
      await invoke('revert_file_changes', { path: filePath });
      setFileActions((prev) => ({ ...prev, [filePath]: 'reverted' }));
      invoke<GitStatusResult>('git_status').then(setGitStatus).catch(() => {});
    } catch (err) {
      console.error('Failed to revert file', err);
      setCommitResult({ success: false, message: `Revert failed: ${String(err)}` });
      setFileActions((prev) => {
        const next = { ...prev };
        delete next[filePath];
        return next;
      });
    }
  };

  const handleStageFile = async (filePath: string) => {
    if (!canMutate) return;
    setFileActions((prev) => ({ ...prev, [filePath]: 'loading' }));
    try {
      await invoke('stage_file_changes', { path: filePath });
      setFileActions((prev) => ({ ...prev, [filePath]: 'accepted' }));
      invoke<GitStatusResult>('git_status').then(setGitStatus).catch(() => {});
    } catch (err) {
      console.error('Failed to stage file', err);
      setCommitResult({ success: false, message: `Stage failed: ${String(err)}` });
      setFileActions((prev) => {
        const next = { ...prev };
        delete next[filePath];
        return next;
      });
    }
  };

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

  const handleCommit = async (push: boolean) => {
    if (!canMutate || !(commitMessage || '').trim()) return;
    setIsCommitting(true);
    setCommitResult(null);
    try {
      const commitOut = await invoke<string>('git_commit', { message: (commitMessage || '').trim() });
      if (push) {
        const pushOut = await invoke<string>('git_push');
        setCommitResult({ success: true, message: `Committed & Pushed: ${pushOut || commitOut}` });
      } else {
        setCommitResult({ success: true, message: `Committed: ${commitOut}` });
      }
      setShowCommitBar(false);
      invoke<GitStatusResult>('git_status').then(setGitStatus).catch(() => {});
    } catch (err) {
      setCommitResult({ success: false, message: String(err) });
    } finally {
      setIsCommitting(false);
    }
  };

  if (files.length === 0) return null;

  return (
    <div className="mt-3 w-full rounded-xl border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-widget-bg)] overflow-hidden">
      {/* Header Bar */}
      <div
        onClick={() => setIsMinimized((prev) => !prev)}
        className={`flex items-center justify-between bg-[var(--ui-widget-bg)] px-3.5 py-2 text-xs select-none cursor-pointer transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)] ${
          isMinimized ? '' : 'border-b border-[var(--ui-stroke-quaternary)]'
        }`}
        title={isMinimized ? 'Click to expand changed files' : 'Click to minimize'}
      >
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              setIsMinimized((prev) => !prev);
            }}
            className="text-[var(--ui-text-tertiary)] hover:text-[var(--ui-text-primary)] p-0.5 rounded transition-colors duration-100 hover:transition-none"
            aria-label={isMinimized ? 'Expand changed files' : 'Minimize changed files'}
          >
            <ChevronDown size={14} className={`transition-transform duration-200 ${isMinimized ? '-rotate-90' : ''}`} />
          </button>
          <GitCompare size={14} className="text-[var(--ui-accent)]" />
          <span className="font-medium text-[var(--ui-text-primary)]">
            {files.length} file{files.length > 1 ? 's' : ''} changed
          </span>
          {gitStatus?.isRepo && (
            <span className="inline-flex items-center gap-1 rounded bg-[var(--ui-control-fill)] px-2 py-0.5 font-mono text-[10px] text-[var(--ui-text-tertiary)]">
              <GitBranch size={10} className="text-[var(--ui-accent)]" />
              {gitStatus.branch}
            </span>
          )}
          {isMinimized && (
            <span className="text-[10px] text-[var(--ui-text-quaternary)] italic">
              (minimized)
            </span>
          )}
        </div>

        <div className="flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
          <div className="flex items-center gap-1.5 font-mono text-[11px]">
            {totalAdditions > 0 && (
              <span className="text-[var(--ui-diff-add-fg)] font-medium">+{totalAdditions}</span>
            )}
            {totalDeletions > 0 && (
              <span className="text-[var(--ui-diff-remove-fg)] font-medium">-{totalDeletions}</span>
            )}
          </div>
          {gitStatus?.isRepo && (
            <button
              type="button"
              disabled={!canMutate}
              onClick={() => {
                if (!canMutate) return;
                if (isMinimized) setIsMinimized(false);
                setShowCommitBar((prev) => !prev);
              }}
              className="flex items-center gap-1 rounded border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] px-2 py-0.5 text-[10px] font-medium text-[var(--ui-text-primary)] hover:border-[var(--ui-accent)] hover:text-[var(--ui-accent)] transition-colors duration-100 hover:transition-none"
              title="Toggle Git Commit Bar"
            >
              <GitCommit size={11} className="text-[var(--ui-accent)]" />
              <span>Commit</span>
              {showCommitBar ? <ChevronUp size={11} /> : <ChevronDown size={11} />}
            </button>
          )}
          <button
            type="button"
            onClick={() => setIsMinimized((prev) => !prev)}
            className="flex items-center gap-1 rounded border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] px-1.5 py-0.5 text-[10px] text-[var(--ui-text-tertiary)] hover:text-[var(--ui-text-primary)] hover:border-[var(--ui-stroke-secondary)] transition-colors duration-100 hover:transition-none"
            title={isMinimized ? 'Expand' : 'Minimize'}
          >
            {isMinimized ? 'Expand' : 'Minimize'}
          </button>
        </div>
      </div>

      {!isMinimized && <p role="note" className="border-b border-[var(--ui-stroke-quaternary)] px-3.5 py-2 text-[11px] text-[var(--ui-text-tertiary)]">{UNSCOPED_RECEIPT_REASON}</p>}
      {/* File List */}
      {!isMinimized && (
        <div className="divide-y divide-[var(--ui-stroke-quaternary)] bg-[var(--ui-bg-chrome)] p-1">
          {files.map(({ stat, artifactId }) => {
          const filename = stat.path.replace(/.*[/\\]/, '');
          const dir = stat.path.includes('/') || stat.path.includes('\\')
            ? stat.path.replace(/[/\\][^/\\]+$/, '')
            : '';

          return (
            <div
              key={stat.path}
              data-file-link="true"
              data-file-path={stat.path}
              className="group flex items-center justify-between rounded-lg px-3 py-2 text-xs transition-colors duration-100 hover:transition-none hover:bg-[var(--ui-row-hover)]"
            >
              <div className="flex items-center gap-2 min-w-0 flex-1 mr-3">
                <FileCode2 size={14} className="text-[var(--ui-text-tertiary)] shrink-0" />
                <span className="truncate font-mono text-[11px] text-[var(--ui-text-secondary)]" title={stat.path}>
                  {dir ? <span className="text-[var(--ui-text-quaternary)] mr-1">{dir}/</span> : null}
                  <span className="text-[var(--ui-text-primary)] font-medium">{filename}</span>
                </span>
              </div>

              <div className="flex items-center gap-2 shrink-0">
                {/* Diff stats badge */}
                <div className="flex items-center gap-1 font-mono text-[10px]">
                  {stat.additions > 0 && (
                    <span className="px-1.5 py-0.5 rounded bg-[var(--ui-diff-add-bg)] text-[var(--ui-diff-add-fg)] font-medium">
                      +{stat.additions}
                    </span>
                  )}
                  {stat.deletions > 0 && (
                    <span className="px-1.5 py-0.5 rounded bg-[var(--ui-diff-remove-bg)] text-[var(--ui-diff-remove-fg)] font-medium">
                      -{stat.deletions}
                    </span>
                  )}
                </div>

                {/* Review Button */}
                {artifactId && onSelectArtifact && (
                  <button
                    type="button"
                    onClick={() => onSelectArtifact(artifactId)}
                    className="flex items-center gap-1 rounded border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] px-2 py-0.5 text-[10px] text-[var(--ui-accent)] hover:bg-[var(--ui-control-hover)] hover:text-[var(--ui-text-primary)] transition-colors duration-100 hover:transition-none"
                    title="Review unified diff"
                  >
                    <span>Review</span>
                    <ExternalLink size={10} />
                  </button>
                )}

                {/* Accept & Revert buttons */}
                {fileActions[stat.path] === 'reverted' ? (
                  <span className="text-[10px] text-[var(--ui-text-tertiary)] font-mono italic">Reverted</span>
                ) : fileActions[stat.path] === 'accepted' ? (
                  <span className="flex items-center gap-1 text-[10px] text-[var(--ui-green)] font-mono">
                    <Check size={11} /> Staged
                  </span>
                ) : (
                  <div className="flex items-center gap-1 opacity-80 group-hover:opacity-100 transition-opacity">
                    <button
                      type="button"
                      disabled={!canMutate || fileActions[stat.path] === 'loading'}
                      onClick={() => handleStageFile(stat.path)}
                      className="flex items-center gap-0.5 rounded border border-[var(--ui-diff-add-border)] bg-[var(--ui-diff-add-bg)] px-1.5 py-0.5 text-[10px] text-[var(--ui-diff-add-fg)] hover:opacity-90 transition-colors"
                      title="Accept / stage this file change"
                    >
                      <Check size={10} />
                      <span>Accept</span>
                    </button>
                    <button
                      type="button"
                      disabled={!canMutate || fileActions[stat.path] === 'loading'}
                      onClick={() => handleRevertFile(stat.path)}
                      className="flex items-center gap-0.5 rounded border border-[var(--ui-diff-remove-border)] bg-[var(--ui-diff-remove-bg)] px-1.5 py-0.5 text-[10px] text-[var(--ui-diff-remove-fg)] hover:opacity-90 transition-colors"
                      title="Revert / discard changes to this file"
                    >
                      <Undo2 size={10} />
                      <span>Revert</span>
                    </button>
                  </div>
                )}
              </div>
            </div>
          );
        })}
      </div>
      )}

      {/* One-Click Git Commit & Push Bar */}
      {!isMinimized && showCommitBar && gitStatus?.isRepo && (
        <div className="border-t border-[var(--ui-stroke-tertiary)] bg-[var(--ui-widget-bg)] p-3 space-y-2">
          <div className="flex items-center gap-2">
            <input
              type="text"
              value={commitMessage}
              onChange={(e) => setCommitMessage(e.target.value)}
              placeholder="Commit message (e.g. feat: add terminal support)..."
              disabled={isCommitting}
              className="flex-1 rounded border border-[var(--ui-stroke-secondary)] bg-[var(--ui-bg-chrome)] px-2.5 py-1.5 font-mono text-xs text-[var(--ui-text-primary)] placeholder:text-[var(--ui-text-quaternary)] outline-none focus:border-[var(--ui-accent)]"
            />
            <button
              type="button"
              onClick={() => handleCommit(false)}
              disabled={isCommitting || !(commitMessage || '').trim()}
              className="flex items-center gap-1.5 rounded bg-[#238636] px-3 py-1.5 text-xs font-medium text-white hover:bg-[#2ea043] disabled:opacity-50 transition-colors shrink-0"
              title="Stage all changes and commit"
            >
              {isCommitting ? <Loader2 size={12} className="animate-spin" /> : <GitCommit size={12} />}
              <span>Commit</span>
            </button>
            <button
              type="button"
              onClick={() => handleCommit(true)}
              disabled={isCommitting || !(commitMessage || '').trim()}
              className="flex items-center gap-1.5 rounded bg-[var(--ui-accent)] px-3 py-1.5 text-xs font-medium text-[var(--ui-accent-fg)] hover:opacity-90 disabled:opacity-50 transition-opacity shrink-0"
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
              ? 'border-[var(--ui-diff-add-border)] bg-[var(--ui-diff-add-bg)] text-[var(--ui-diff-add-fg)]'
              : 'border-[var(--ui-diff-remove-border)] bg-[var(--ui-diff-remove-bg)] text-[var(--ui-diff-remove-fg)]'
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
