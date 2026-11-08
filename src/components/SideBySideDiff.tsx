import React, { useMemo, useRef } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';

interface SideBySideDiffProps {
  diffText: string;
}

interface DiffLine {
  oldLineNumber?: number;
  newLineNumber?: number;
  oldContent?: string;
  newContent?: string;
  type: 'unchanged' | 'modified' | 'added' | 'deleted' | 'hunk';
}

/**
 * Basic word-level diff highlight helper.
 * Highlights words that differ between old and new strings.
 */
function renderWordDiff(oldText: string, newText: string, isNew: boolean): React.ReactNode {
  if (!oldText && !newText) return ' ';
  if (!oldText) return <span className="bg-[#238636]/35 rounded px-0.5">{newText}</span>;
  if (!newText) return <span className="bg-[#da3633]/35 rounded px-0.5">{oldText}</span>;

  const oldWords = oldText.split(/(\s+|[^\w\s])/);
  const newWords = newText.split(/(\s+|[^\w\s])/);

  const targetWords = isNew ? newWords : oldWords;
  const compareWords = isNew ? new Set(oldWords) : new Set(newWords);

  return targetWords.map((word, idx) => {
    const isDifferent = (word || '').trim().length > 0 && !compareWords.has(word);
    if (!isDifferent) {
      return <span key={idx}>{word}</span>;
    }
    return (
      <span
        key={idx}
        className={isNew ? 'bg-[#238636]/40 font-semibold rounded px-0.5' : 'bg-[#da3633]/40 font-semibold rounded px-0.5'}
      >
        {word}
      </span>
    );
  });
}

export const SideBySideDiff: React.FC<SideBySideDiffProps> = ({ diffText }) => {
  const diffRows = useMemo(() => {
    const rawLines = diffText.split('\n');
    const rows: DiffLine[] = [];

    let oldLineNum = 1;
    let newLineNum = 1;
    let oldRemaining = 0;
    let newRemaining = 0;

    let pendingOld: { num: number; text: string }[] = [];
    let pendingNew: { num: number; text: string }[] = [];

    const flushPending = () => {
      const maxLen = Math.max(pendingOld.length, pendingNew.length);
      for (let i = 0; i < maxLen; i++) {
        const oldItem = pendingOld[i];
        const newItem = pendingNew[i];

        if (oldItem && newItem) {
          rows.push({
            oldLineNumber: oldItem.num,
            newLineNumber: newItem.num,
            oldContent: oldItem.text,
            newContent: newItem.text,
            type: 'modified',
          });
        } else if (oldItem) {
          rows.push({
            oldLineNumber: oldItem.num,
            oldContent: oldItem.text,
            type: 'deleted',
          });
        } else if (newItem) {
          rows.push({
            newLineNumber: newItem.num,
            newContent: newItem.text,
            type: 'added',
          });
        }
      }
      pendingOld = [];
      pendingNew = [];
    };

    for (const rawLine of rawLines) {
      const match = rawLine.match(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/);
      if (match) {
        flushPending();
        oldLineNum = Number(match[1]);
        newLineNum = Number(match[3]);
        oldRemaining = match[2] === undefined ? 1 : Number(match[2]);
        newRemaining = match[4] === undefined ? 1 : Number(match[4]);
        rows.push({ oldContent: rawLine, newContent: rawLine, type: 'hunk' });
        continue;
      }
      // Markers are metadata, not source lines, and do not split a change pair.
      if (rawLine.startsWith('\\ No newline at end of file')) continue;
      if (oldRemaining === 0 && newRemaining === 0) {
        flushPending();
        continue;
      }

      if (rawLine.startsWith('-') && oldRemaining > 0) {
        pendingOld.push({ num: oldLineNum++, text: rawLine.slice(1) });
        oldRemaining--;
      } else if (rawLine.startsWith('+') && newRemaining > 0) {
        pendingNew.push({ num: newLineNum++, text: rawLine.slice(1) });
        newRemaining--;
      } else if (rawLine.startsWith(' ') && oldRemaining > 0 && newRemaining > 0) {
        flushPending();
        const content = rawLine.slice(1);
        rows.push({
          oldLineNumber: oldLineNum++,
          newLineNumber: newLineNum++,
          oldContent: content,
          newContent: content,
          type: 'unchanged',
        });
        oldRemaining--;
        newRemaining--;
      }
    }

    flushPending();
    return rows;
  }, [diffText]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: diffRows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 20,
    overscan: 20,
  });

  return (
    <div className="flex h-full w-full flex-col bg-[#0d1117] font-mono text-[11px] leading-5 select-text">
      <div className="flex shrink-0 border-b border-[#30363d] bg-[#161b22] text-[10px] uppercase font-semibold text-[#8b949e]">
        <div className="flex min-w-0 flex-1 items-center justify-between border-r border-[#30363d] px-3 py-1.5"><span>Original (Before)</span><span className="text-[#f85149]">- deletions</span></div>
        <div className="flex min-w-0 flex-1 items-center justify-between px-3 py-1.5"><span>Modified (After)</span><span className="text-[#3fb950]">+ additions</span></div>
      </div>
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-auto">
        <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
          {virtualizer.getVirtualItems().map((virtualRow) => {
            const row = diffRows[virtualRow.index];
            const isModified = row.type === 'modified';
            const isDeleted = row.type === 'deleted';
            const isAdded = row.type === 'added';
            return (
              <div key={virtualRow.key} data-index={virtualRow.index} ref={virtualizer.measureElement} className="absolute left-0 top-0 w-full" style={{ transform: `translateY(${virtualRow.start}px)` }}>
                {row.type === 'hunk' ? (
                  <div className="flex bg-[#161b22]/70 px-3 py-1 text-[10px] text-[#79c0ff]"><span className="w-full text-center truncate">{row.oldContent}</span></div>
                ) : (
                  <div className="flex min-h-[20px] w-full border-b border-[#21262d]/40 hover:bg-[#161b22]/30">
                    <div className={`flex min-w-0 flex-1 border-r border-[#21262d]/60 ${isModified || isDeleted ? 'bg-[#da3633]/15 text-[#ff7b72]' : 'text-[#8b949e]'}`}>
                      <span className="w-10 shrink-0 select-none border-r border-[#21262d]/60 pr-2 text-right text-[#484f58]">{row.oldLineNumber ?? ''}</span>
                      <span className="flex-1 whitespace-pre-wrap break-all px-2">{isModified ? renderWordDiff(row.oldContent || '', row.newContent || '', false) : row.oldContent || (isAdded ? '' : ' ')}</span>
                    </div>
                    <div className={`flex min-w-0 flex-1 ${isModified || isAdded ? 'bg-[#238636]/15 text-[#7ee787]' : 'text-[#c9d1d9]'}`}>
                      <span className="w-10 shrink-0 select-none border-r border-[#21262d]/60 pr-2 text-right text-[#484f58]">{row.newLineNumber ?? ''}</span>
                      <span className="flex-1 whitespace-pre-wrap break-all px-2">{isModified ? renderWordDiff(row.oldContent || '', row.newContent || '', true) : row.newContent || (isDeleted ? '' : ' ')}</span>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
};
