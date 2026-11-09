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
        <div className="flex items-center gap-1.5 font-mono text-xs font-medium text-[#8b949e]">
          <span>Comparison</span>
        </div>
        {chosenWinner && (
          <span className="flex items-center gap-1 rounded bg-[#238636]/15 px-2 py-0.5 font-mono text-[10px] font-medium text-[#3fb950] border border-[#238636]/30">
            <Check size={11} /> Model {chosenWinner} selected
          </span>
        )}
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
        {/* Model A Card */}
        <div
          className={`flex flex-col rounded-lg border bg-[#0d1117] p-3 shadow-sm transition-all ${
            chosenWinner === 'A'
              ? 'border-[#238636]/60 bg-[#238636]/5 ring-1 ring-[#238636]/30'
              : 'border-[#30363d] hover:border-[#58a6ff]/40'
          }`}
        >
          {/* Candidate A Header */}
          <div className="mb-2 flex items-center justify-between border-b border-[#21262d] pb-2 text-xs">
            <div className="flex items-center gap-2 min-w-0">
              <span className="flex h-4 w-4 items-center justify-center rounded bg-[#1f6feb]/20 font-mono text-[10px] font-bold text-[#58a6ff]">
                A
              </span>
              <span className="truncate font-mono text-xs text-[#e6edf3]" title={candidateA.model}>
                {candidateA.model}
              </span>
              {candidateA.durationMs && (
                <span className="font-mono text-[10px] text-[#6e7681]">{(candidateA.durationMs / 1000).toFixed(1)}s</span>
              )}
            </div>
            {onSelectWinner && (
              <button
                type="button"
                disabled={selectionDisabled}
                onClick={() => onSelectWinner('A')}
                className={`flex items-center gap-1 rounded px-2 py-0.5 font-mono text-[10px] transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
                  chosenWinner === 'A'
                    ? 'bg-[#238636] text-white'
                    : 'border border-[#30363d] bg-[#161b22] text-[#8b949e] hover:border-[#58a6ff] hover:text-[#e6edf3]'
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
            <details className="mb-2 rounded border border-[#30363d] bg-[#161b22] p-2 text-[11px] text-[#8b949e]">
              <summary className="cursor-pointer text-[#58a6ff] font-medium font-mono text-[10px]">Reasoning</summary>
              <div className="mt-1 whitespace-pre-wrap">{candidateA.reasoning}</div>
            </details>
          )}

          {/* Content */}
          <div className="flex-1 text-[13px] leading-relaxed text-[#e6edf3]">
            {candidateA.content ? (
              <MarkdownRenderer content={candidateA.content} />
            ) : candidateA.isStreaming ? (
              <span className="text-[#8b949e] italic animate-pulse font-mono text-xs">Generating response...</span>
            ) : (
              <span className="text-[#6e7681] italic font-mono text-xs">No output</span>
            )}
          </div>
        </div>

        {/* Model B Card */}
        <div
          className={`flex flex-col rounded-lg border bg-[#0d1117] p-3 shadow-sm transition-all ${
            chosenWinner === 'B'
              ? 'border-[#238636]/60 bg-[#238636]/5 ring-1 ring-[#238636]/30'
              : 'border-[#30363d] hover:border-[#d29922]/40'
          }`}
        >
          {/* Candidate B Header */}
          <div className="mb-2 flex items-center justify-between border-b border-[#21262d] pb-2 text-xs">
            <div className="flex items-center gap-2 min-w-0">
              <span className="flex h-4 w-4 items-center justify-center rounded bg-[#d29922]/20 font-mono text-[10px] font-bold text-[#d29922]">
                B
              </span>
              <span className="truncate font-mono text-xs text-[#e6edf3]" title={candidateB.model}>
                {candidateB.model}
              </span>
              {candidateB.durationMs && (
                <span className="font-mono text-[10px] text-[#6e7681]">{(candidateB.durationMs / 1000).toFixed(1)}s</span>
              )}
            </div>
            {onSelectWinner && (
              <button
                type="button"
                disabled={selectionDisabled}
                onClick={() => onSelectWinner('B')}
                className={`flex items-center gap-1 rounded px-2 py-0.5 font-mono text-[10px] transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
                  chosenWinner === 'B'
                    ? 'bg-[#238636] text-white'
                    : 'border border-[#30363d] bg-[#161b22] text-[#8b949e] hover:border-[#d29922] hover:text-[#e6edf3]'
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
            <details className="mb-2 rounded border border-[#30363d] bg-[#161b22] p-2 text-[11px] text-[#8b949e]">
              <summary className="cursor-pointer text-[#d29922] font-medium font-mono text-[10px]">Reasoning</summary>
              <div className="mt-1 whitespace-pre-wrap">{candidateB.reasoning}</div>
            </details>
          )}

          {/* Content */}
          <div className="flex-1 text-[13px] leading-relaxed text-[#e6edf3]">
            {candidateB.content ? (
              <MarkdownRenderer content={candidateB.content} />
            ) : candidateB.isStreaming ? (
              <span className="text-[#8b949e] italic animate-pulse font-mono text-xs">Generating response...</span>
            ) : (
              <span className="text-[#6e7681] italic font-mono text-xs">No output</span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
