import React from 'react';
import { Check } from 'lucide-react';
import type { CouncilTurn } from '../types/agent';
import { MarkdownRenderer } from './MarkdownRenderer';

interface CouncilViewProps {
  councilTurn: CouncilTurn;
  isStreaming?: boolean;
  onSelectWinner?: (winner: 'A' | 'B') => void;
}

export const CouncilView: React.FC<CouncilViewProps> = ({
  councilTurn,
  isStreaming = false,
  onSelectWinner,
}) => {
  const { candidateA, candidateB, chosenWinner } = councilTurn;
  const selectionDisabled = Boolean(isStreaming || candidateA.isStreaming || candidateB.isStreaming);

  return (
    <div className="my-3 w-full space-y-2">
      <div className="flex items-center justify-between px-1">
        <div className="flex items-center gap-1.5 font-mono text-xs font-medium text-[var(--ui-text-tertiary)]">
          <span>Comparison</span>
        </div>
        {chosenWinner && (
          <span className="flex items-center gap-1 rounded bg-[var(--ui-diff-add-bg)] px-2 py-0.5 font-mono text-[10px] font-medium text-[var(--ui-diff-add-fg)] border border-[var(--ui-diff-add-border)]">
            <Check size={11} /> Model {chosenWinner} selected
          </span>
        )}
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
        {/* Model A Card */}
        <div
          className={`flex flex-col rounded-xl border p-3 transition-colors ${
            chosenWinner === 'A'
              ? 'border-[var(--ui-diff-add-border)] bg-[var(--ui-diff-add-bg)]'
              : 'border-[var(--ui-stroke-tertiary)] bg-[var(--ui-widget-bg)] hover:border-[var(--ui-accent-border)]'
          }`}
        >
          {/* Candidate A Header */}
          <div className="mb-2 flex items-center justify-between border-b border-[var(--ui-stroke-quaternary)] pb-2 text-xs">
            <div className="flex items-center gap-2 min-w-0">
              <span className="flex h-4 w-4 items-center justify-center rounded bg-[var(--ui-row-active)] font-mono text-[10px] font-bold text-[var(--ui-accent)]">
                A
              </span>
              <span className="truncate font-mono text-xs text-[var(--ui-text-primary)]" title={candidateA.model}>
                {candidateA.model}
              </span>
              {candidateA.durationMs && (
                <span className="font-mono text-[10px] text-[var(--ui-text-quaternary)]">{(candidateA.durationMs / 1000).toFixed(1)}s</span>
              )}
            </div>
            {onSelectWinner && (
              <button
                type="button"
                disabled={selectionDisabled}
                onClick={() => onSelectWinner('A')}
                className={`flex items-center gap-1 rounded px-2 py-0.5 font-mono text-[10px] transition-colors duration-100 hover:transition-none disabled:cursor-not-allowed disabled:opacity-50 ${
                  chosenWinner === 'A'
                    ? 'bg-[var(--ui-green)] text-white'
                    : 'border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] text-[var(--ui-text-tertiary)] hover:border-[var(--ui-accent)] hover:text-[var(--ui-text-primary)]'
                }`}
                title="Use Model A's response as the active chat answer"
              >
                {chosenWinner === 'A' && <Check size={11} />}
                <span>{chosenWinner === 'A' ? 'Selected' : 'Use response'}</span>
              </button>
            )}
          </div>

          {/* Reasoning trace if present */}
          {candidateA.reasoning && (
            <details className="mb-2 rounded border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-chrome)] p-2 text-[11px] text-[var(--conversation-scaffold-text)]">
              <summary className="cursor-pointer text-[var(--ui-accent)] font-medium font-mono text-[10px]">Reasoning</summary>
              <div className="mt-1 whitespace-pre-wrap">{candidateA.reasoning}</div>
            </details>
          )}

          {/* Content */}
          <div className="flex-1 text-[13px] leading-relaxed text-[var(--ui-text-primary)]">
            {candidateA.content ? (
              <MarkdownRenderer content={candidateA.content} />
            ) : candidateA.isStreaming ? (
              <span className="text-[var(--ui-text-tertiary)] italic animate-pulse font-mono text-xs">Generating response...</span>
            ) : (
              <span className="text-[var(--ui-text-quaternary)] italic font-mono text-xs">No output</span>
            )}
          </div>
        </div>

        {/* Model B Card */}
        <div
          className={`flex flex-col rounded-xl border p-3 transition-colors ${
            chosenWinner === 'B'
              ? 'border-[var(--ui-diff-add-border)] bg-[var(--ui-diff-add-bg)]'
              : 'border-[var(--ui-stroke-tertiary)] bg-[var(--ui-widget-bg)] hover:border-[var(--ui-warn-border)]'
          }`}
        >
          {/* Candidate B Header */}
          <div className="mb-2 flex items-center justify-between border-b border-[var(--ui-stroke-quaternary)] pb-2 text-xs">
            <div className="flex items-center gap-2 min-w-0">
              <span className="flex h-4 w-4 items-center justify-center rounded bg-[var(--ui-warn-bg)] font-mono text-[10px] font-bold text-[var(--ui-yellow)]">
                B
              </span>
              <span className="truncate font-mono text-xs text-[var(--ui-text-primary)]" title={candidateB.model}>
                {candidateB.model}
              </span>
              {candidateB.durationMs && (
                <span className="font-mono text-[10px] text-[var(--ui-text-quaternary)]">{(candidateB.durationMs / 1000).toFixed(1)}s</span>
              )}
            </div>
            {onSelectWinner && (
              <button
                type="button"
                disabled={selectionDisabled}
                onClick={() => onSelectWinner('B')}
                className={`flex items-center gap-1 rounded px-2 py-0.5 font-mono text-[10px] transition-colors duration-100 hover:transition-none disabled:cursor-not-allowed disabled:opacity-50 ${
                  chosenWinner === 'B'
                    ? 'bg-[var(--ui-green)] text-white'
                    : 'border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] text-[var(--ui-text-tertiary)] hover:border-[var(--ui-yellow)] hover:text-[var(--ui-text-primary)]'
                }`}
                title="Use Model B's response as the active chat answer"
              >
                {chosenWinner === 'B' && <Check size={11} />}
                <span>{chosenWinner === 'B' ? 'Selected' : 'Use response'}</span>
              </button>
            )}
          </div>

          {/* Reasoning trace if present */}
          {candidateB.reasoning && (
            <details className="mb-2 rounded border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-bg-chrome)] p-2 text-[11px] text-[var(--conversation-scaffold-text)]">
              <summary className="cursor-pointer text-[var(--ui-yellow)] font-medium font-mono text-[10px]">Reasoning</summary>
              <div className="mt-1 whitespace-pre-wrap">{candidateB.reasoning}</div>
            </details>
          )}

          {/* Content */}
          <div className="flex-1 text-[13px] leading-relaxed text-[var(--ui-text-primary)]">
            {candidateB.content ? (
              <MarkdownRenderer content={candidateB.content} />
            ) : candidateB.isStreaming ? (
              <span className="text-[var(--ui-text-tertiary)] italic animate-pulse font-mono text-xs">Generating response...</span>
            ) : (
              <span className="text-[var(--ui-text-quaternary)] italic font-mono text-xs">No output</span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
