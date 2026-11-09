import React, { useEffect, useMemo, useState } from 'react';
import { CheckCircle2, Circle, Clock, ListTodo } from 'lucide-react';

interface PlanItem {
  id: string;
  status: 'done' | 'in_progress' | 'pending';
  text: string;
  sourceStatus: PlanItem['status'];
}

export const InteractivePlanCard: React.FC<{ rawPlan: string }> = ({ rawPlan }) => {
  const initialItems = useMemo(() => {
    const occurrences = new Map<string, number>();
    return rawPlan.split('\n').filter((line) => line.trim().length > 0).map((line): PlanItem => {
      const match = line.match(/^\s*(?:[-*]|\d+\.)\s*\[([ xX\-~])\]\s*(.*)$/);
      const mark = match?.[1].trim().toLowerCase();
      const status = mark === 'x' ? 'done' : mark === '-' || mark === '~' ? 'in_progress' : 'pending';
      const text = match ? match[2].trim() : line.replace(/^\s*(?:[-*]|\d+\.)\s*/, '').trim();
      const occurrence = occurrences.get(text) ?? 0;
      occurrences.set(text, occurrence + 1);
      return { id: text + '#' + occurrence, status, sourceStatus: status, text };
    });
  }, [rawPlan]);

  const [items, setItems] = useState<PlanItem[]>(initialItems);

  useEffect(() => {
    setItems((previous) => {
      const previousById = new Map(previous.map((item) => [item.id, item]));
      const next = initialItems.map((incoming) => {
        const existing = previousById.get(incoming.id);
        // Keep local toggles only while the source status remains unchanged.
        return existing?.sourceStatus === incoming.sourceStatus ? existing : incoming;
      });
      return next.length === previous.length && next.every((item, index) => item === previous[index])
        ? previous
        : next;
    });
  }, [initialItems]);

  const toggleItem = (id: string) => {
    setItems((prev) =>
      prev.map((item) => {
        if (item.id !== id) return item;
        const nextStatus = item.status === 'done' ? 'pending' : 'done';
        return { ...item, status: nextStatus };
      }),
    );
  };

  const doneCount = items.filter((i) => i.status === 'done').length;
  const progressPercent = items.length > 0 ? Math.round((doneCount / items.length) * 100) : 0;

  return (
    <div
      className="my-3 overflow-hidden rounded-lg border border-[#30363d] bg-[#161b22] shadow-sm"
      data-testid="interactive-plan-card"
    >
      <div className="border-b border-[#30363d]/60 bg-[#161b22] px-3.5 py-2.5">
        <div className="flex items-center justify-between text-xs">
          <div className="flex items-center gap-2">
            <ListTodo size={14} className="text-[#58a6ff]" />
            <span className="font-semibold text-[#f0f6fc]">Execution Plan</span>
            <span className="rounded-full bg-[#30363d] px-2 py-0.5 font-mono text-[10px] text-[#8b949e]">
              {doneCount}/{items.length} completed
            </span>
          </div>
          <span className="font-mono text-[11px] font-medium text-[#58a6ff]">{progressPercent}%</span>
        </div>
        {/* Progress bar */}
        <div
          role="progressbar"
          aria-label="Plan progress"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={progressPercent}
          className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-[#21262d]"
        >
          <div
            className="h-full rounded-full bg-[#58a6ff] transition-[width] duration-[160ms] ease-out"
            style={{ width: `${progressPercent}%` }}
          />
        </div>
      </div>

      <div className="divide-y divide-[#21262d]/50 p-1.5 bg-[#0d1117]">
        {items.map((item) => (
          <button
            key={item.id}
            type="button"
            role="checkbox"
            aria-checked={item.status === 'done'}
            aria-label={item.text}
            onClick={() => toggleItem(item.id)}
            className="flex w-full items-start gap-2.5 px-2.5 py-1.5 text-left text-xs cursor-pointer hover:bg-[#161b22]/60 rounded-lg transition-colors group select-none focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#58a6ff]"
          >
            <span className="mt-0.5 shrink-0" aria-hidden="true">
              {item.status === 'done' ? (
                <CheckCircle2 size={13} className="text-[#3fb950]" />
              ) : item.status === 'in_progress' ? (
                <Clock size={13} className="text-[#f0883e]" />
              ) : (
                <Circle size={13} className="text-[#6e7681] group-hover:text-[#c9d1d9]" />
              )}
            </span>
            <span
              className={`leading-relaxed text-[11px] font-sans ${
                item.status === 'done'
                  ? 'text-[#8b949e] line-through decoration-[#8b949e]/50'
                  : item.status === 'in_progress'
                  ? 'text-[#f0883e] font-medium'
                  : 'text-[#e6edf3]'
              }`}
            >
              {item.text}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
};
