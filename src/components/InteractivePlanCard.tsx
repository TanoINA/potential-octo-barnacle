import React, { useState } from 'react';
import { CheckCircle2, Circle, Clock, ListTodo } from 'lucide-react';

interface PlanItem {
  id: string;
  status: 'done' | 'in_progress' | 'pending';
  text: string;
}

export const InteractivePlanCard: React.FC<{ rawPlan: string }> = ({ rawPlan }) => {
  const lines = rawPlan.split('\n').filter((l) => l.trim().length > 0);

  const initialItems: PlanItem[] = lines.map((line, idx) => {
    const match = line.match(/^\s*(?:[-*]|\d+\.)\s*\[([ xX\-~])\]\s*(.*)$/);
    if (match) {
      const mark = match[1].trim();
      const status = mark.toLowerCase() === 'x' ? 'done' : mark === '-' || mark === '~' ? 'in_progress' : 'pending';
      return { id: `item-${idx}`, status, text: match[2].trim() };
    }
    return { id: `item-${idx}`, status: 'pending', text: line.replace(/^\s*(?:[-*]|\d+\.)\s*/, '').trim() };
  });

  const [items, setItems] = useState<PlanItem[]>(initialItems);

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
      className="my-3 overflow-hidden rounded-xl border border-[#30363d] bg-[#161b22]/90 shadow-md backdrop-blur-sm"
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
        <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-[#21262d]">
          <div
            className="h-full rounded-full bg-gradient-to-r from-[#2ea043] to-[#58a6ff] transition-all duration-300"
            style={{ width: `${progressPercent}%` }}
          />
        </div>
      </div>

      <div className="divide-y divide-[#21262d]/50 p-1.5 bg-[#0d1117]">
        {items.map((item) => (
          <div
            key={item.id}
            onClick={() => toggleItem(item.id)}
            className="flex items-start gap-2.5 px-2.5 py-1.5 text-xs cursor-pointer hover:bg-[#161b22]/60 rounded-lg transition-colors group select-none"
          >
            <div className="mt-0.5 shrink-0">
              {item.status === 'done' ? (
                <CheckCircle2 size={13} className="text-[#3fb950]" />
              ) : item.status === 'in_progress' ? (
                <Clock size={13} className="text-[#f0883e] animate-pulse" />
              ) : (
                <Circle size={13} className="text-[#6e7681] group-hover:text-[#c9d1d9]" />
              )}
            </div>
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
          </div>
        ))}
      </div>
    </div>
  );
};
