import React, { memo, useEffect, useMemo, useRef, useState } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import type { ChatMessage } from '../types/agent';

interface TimelineTurn {
  id: string;
  turnIndex: number;
  messageIndex: number;
  timeStr: string;
  dateStr: string;
  timestamp: number;
  snippet: string;
}

interface TimelineScrubberProps {
  messages: ChatMessage[];
  sessionTitle: string;
  scrollElement: HTMLElement | null;
  onJumpToMessage: (index: number) => void;
}

function formatTurnTime(ts: number): string {
  try {
    return new Date(ts).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  } catch {
    return '';
  }
}

function formatDate(ts: number): string {
  try {
    return new Date(ts).toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' });
  } catch {
    return '';
  }
}

export const TimelineScrubber = memo(function TimelineScrubber({
  messages,
  sessionTitle,
  scrollElement,
  onJumpToMessage,
}: TimelineScrubberProps) {
  const [scrollRatio, setScrollRatio] = useState(0);
  const [isHovered, setIsHovered] = useState(false);
  const trackRef = useRef<HTMLDivElement>(null);
  const hideTimerRef = useRef<number | null>(null);

  // Extract user turns
  const turns: TimelineTurn[] = useMemo(() => {
    const list: TimelineTurn[] = [];
    let count = 0;
    messages.forEach((msg, idx) => {
      if (msg.role === 'user') {
        count += 1;
        const text = (msg.content || '').trim();
        const snippet = text
          ? text.split('\n')[0].slice(0, 48)
          : msg.attachments && msg.attachments.length > 0
          ? `${msg.attachments.length} attachment${msg.attachments.length > 1 ? 's' : ''}`
          : 'User prompt';

        list.push({
          id: msg.id,
          turnIndex: count,
          messageIndex: idx,
          timeStr: formatTurnTime(msg.timestamp),
          dateStr: formatDate(msg.timestamp),
          timestamp: msg.timestamp,
          snippet,
        });
      }
    });
    return list;
  }, [messages]);

  // Track scroll position
  useEffect(() => {
    if (!scrollElement) return;

    const onScroll = () => {
      const { scrollTop, scrollHeight, clientHeight } = scrollElement;
      const maxScroll = scrollHeight - clientHeight;
      if (maxScroll <= 0) {
        setScrollRatio(0);
      } else {
        const ratio = Math.max(0, Math.min(1, scrollTop / maxScroll));
        setScrollRatio(ratio);
      }
    };

    onScroll();
    scrollElement.addEventListener('scroll', onScroll, { passive: true });
    return () => scrollElement.removeEventListener('scroll', onScroll);
  }, [scrollElement, messages.length]);

  // Compute date header range
  const dateSubtitle = useMemo(() => {
    if (turns.length === 0) return '';
    const first = turns[0];
    const last = turns[turns.length - 1];
    if (turns.length === 1 || first.dateStr === last.dateStr) {
      if (first.timeStr === last.timeStr) {
        return `${first.dateStr}, ${first.timeStr}`;
      }
      return `${first.dateStr}, ${first.timeStr} – ${last.timeStr}`;
    }
    return `${first.dateStr} – ${last.dateStr}`;
  }, [turns]);

  // Only render when there are at least 2 turns to scrub through
  if (turns.length < 2) return null;

  const handleMouseEnter = () => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    setIsHovered(true);
  };

  const handleMouseLeave = () => {
    if (hideTimerRef.current !== null) window.clearTimeout(hideTimerRef.current);
    hideTimerRef.current = window.setTimeout(() => {
      setIsHovered(false);
    }, 250);
  };

  const handleTrackClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!trackRef.current) return;
    const rect = trackRef.current.getBoundingClientRect();
    const offsetY = e.clientY - rect.top;
    const ratio = Math.max(0, Math.min(1, offsetY / rect.height));

    // Jump to the closest turn
    const targetTurnIdx = Math.round(ratio * (turns.length - 1));
    const targetTurn = turns[targetTurnIdx];
    if (targetTurn) {
      onJumpToMessage(targetTurn.messageIndex);
    }
  };

  // Approximate current turn index from scroll ratio
  const activeTurnIndex = Math.min(
    turns.length - 1,
    Math.round(scrollRatio * (turns.length - 1)),
  );

  return (
    <div
      className="absolute right-2 top-20 bottom-28 z-30 flex flex-col items-center justify-center select-none pointer-events-auto"
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
      data-testid="timeline-scrubber"
    >
      {/* Top arrow button (jump to top) */}
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onJumpToMessage(0);
        }}
        className="mb-1 p-0.5 text-[#6e7681] hover:text-[#e6edf3] transition-colors rounded hover:bg-[#30363d]/40"
        title="Jump to top"
        aria-label="Jump to top"
      >
        <ChevronUp size={11} strokeWidth={2.5} />
      </button>

      {/* Vertical Dashed & Dotted Track */}
      <div
        ref={trackRef}
        onClick={handleTrackClick}
        className="relative flex h-full max-h-[380px] w-5 cursor-pointer flex-col items-center justify-between py-1 group"
        title="Session timeline scrubber"
      >
        {/* Vertical line connector */}
        <div className="absolute inset-y-2 w-[1px] bg-gradient-to-b from-transparent via-[#30363d]/80 to-transparent" />

        {/* Ticks: dashes and dots */}
        {turns.map((turn, idx) => {
          const isActive = idx === activeTurnIndex;
          const isKeyTurn = idx === 0 || idx === turns.length - 1 || idx % 2 === 0;

          return (
            <div
              key={turn.id}
              onClick={(e) => {
                e.stopPropagation();
                onJumpToMessage(turn.messageIndex);
              }}
              className="relative z-10 flex items-center justify-center py-0.5 hover:scale-125 transition-transform"
              title={`${turn.timeStr}: ${turn.snippet}`}
            >
              {isKeyTurn ? (
                <div
                  className={`h-2.5 w-0.5 rounded-full transition-colors ${
                    isActive ? 'bg-[#58a6ff] w-1 shadow-sm' : 'bg-[#484f58] group-hover:bg-[#8b949e]'
                  }`}
                />
              ) : (
                <div
                  className={`h-1.5 w-1.5 rounded-full transition-colors ${
                    isActive ? 'bg-[#58a6ff] scale-125' : 'bg-[#30363d] group-hover:bg-[#6e7681]'
                  }`}
                />
              )}
            </div>
          );
        })}

        {/* Dynamic White Scrubber Thumb */}
        <div
          className="pointer-events-none absolute left-1/2 h-3.5 w-1 -translate-x-1/2 rounded-full bg-white shadow-md transition-all duration-75"
          style={{
            top: `calc(${scrollRatio * 100}% - 7px)`,
          }}
        />
      </div>

      {/* Bottom arrow button (jump to bottom) */}
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          if (messages.length > 0) {
            onJumpToMessage(messages.length - 1);
          }
        }}
        className="mt-1 p-0.5 text-[#6e7681] hover:text-[#e6edf3] transition-colors rounded hover:bg-[#30363d]/40"
        title="Jump to bottom"
        aria-label="Jump to bottom"
      >
        <ChevronDown size={11} strokeWidth={2.5} />
      </button>

      {/* Floating Hover Card (Antigravity Style Popover) */}
      {isHovered && (
        <div
          className="absolute right-7 top-1/2 -translate-y-1/2 w-80 max-w-[min(90vw,340px)] rounded-2xl border border-[#30363d] bg-[#161b22]/98 p-4 shadow-2xl backdrop-blur-md text-[#e6edf3] animate-fadeIn"
          data-testid="timeline-popover"
        >
          {/* Header */}
          <div className="border-b border-[#21262d] pb-2.5 mb-2">
            <h4 className="text-xs font-semibold text-[#f0f6fc] truncate" title={sessionTitle}>
              {sessionTitle || 'Conversation'}
            </h4>
            <div className="mt-1 flex items-center gap-1.5 font-mono text-[10px] text-[#8b949e]">
              <span>{dateSubtitle}</span>
              <span>·</span>
              <span className="text-[#c9d1d9] font-medium">{turns.length} turns</span>
            </div>
          </div>

          {/* Turn List */}
          <div className="max-h-60 overflow-y-auto space-y-0.5 pr-1 font-sans text-xs">
            {turns.map((turn, idx) => {
              const isSelected = idx === activeTurnIndex;
              return (
                <div
                  key={turn.id}
                  onClick={() => {
                    onJumpToMessage(turn.messageIndex);
                  }}
                  className={`group/item flex items-center gap-2.5 rounded-lg px-2 py-1.5 cursor-pointer transition-colors ${
                    isSelected
                      ? 'bg-[#21262d] text-[#58a6ff]'
                      : 'hover:bg-[#21262d]/60 text-[#c9d1d9]'
                  }`}
                >
                  <span className="shrink-0 font-mono text-[10px] text-[#8b949e] w-14">
                    {turn.timeStr}
                  </span>
                  <span className="truncate text-[11px] leading-relaxed group-hover/item:text-white">
                    {turn.snippet}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
});
