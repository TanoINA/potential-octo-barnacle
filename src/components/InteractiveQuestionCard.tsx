import React, { useState } from 'react';
import { HelpCircle, Check, Send } from 'lucide-react';

export interface QuestionItem {
  question: string;
  options: string[];
  is_multi_select?: boolean;
}

export interface InteractiveQuestionCardProps {
  questions: QuestionItem[];
  onSelectOption?: (answer: string) => void;
  disabled?: boolean;
}

export const InteractiveQuestionCard: React.FC<InteractiveQuestionCardProps> = ({
  questions,
  onSelectOption,
  disabled = false,
}) => {
  const [selectedAnswers, setSelectedAnswers] = useState<Record<number, string[]>>({});
  const [submitted, setSubmitted] = useState(false);

  if (!questions || questions.length === 0) return null;

  const handleToggleOption = (qIndex: number, option: string, isMulti?: boolean) => {
    if (disabled || submitted) return;

    if (!isMulti) {
      setSelectedAnswers((prev) => ({ ...prev, [qIndex]: [option] }));
      if (onSelectOption) {
        setSubmitted(true);
        onSelectOption(option);
      }
      return;
    }

    setSelectedAnswers((prev) => {
      const current = prev[qIndex] || [];
      const next = current.includes(option)
        ? current.filter((item) => item !== option)
        : [...current, option];
      return { ...prev, [qIndex]: next };
    });
  };

  const handleMultiSubmit = (qIndex: number) => {
    if (disabled || submitted || !onSelectOption) return;
    const chosen = selectedAnswers[qIndex] || [];
    if (chosen.length === 0) return;
    setSubmitted(true);
    onSelectOption(chosen.join(', '));
  };

  return (
    <div
      className="my-3 overflow-hidden rounded-xl border border-[var(--ui-stroke-tertiary)] bg-[var(--ui-widget-bg)] max-w-2xl text-xs"
      data-testid="interactive-question-card"
    >
      <div className="flex items-center gap-2 border-b border-[var(--ui-stroke-quaternary)] bg-[var(--ui-widget-bg)] px-3.5 py-2.5">
        <HelpCircle size={14} className="text-[var(--ui-accent)] shrink-0" />
        <span className="font-semibold text-[var(--ui-text-primary)]">Clarification Needed</span>
        <span className="rounded-full bg-[var(--ui-control-fill)] px-2 py-0.5 font-mono text-[10px] text-[var(--ui-text-tertiary)]">
          {questions.length} {questions.length === 1 ? 'question' : 'questions'}
        </span>
      </div>

      <div className="divide-y divide-[var(--ui-stroke-quaternary)] p-3.5 space-y-3.5">
        {questions.map((q, qIndex) => {
          const isMulti = Boolean(q.is_multi_select);
          const chosen = selectedAnswers[qIndex] || [];

          return (
            <div key={qIndex} className={qIndex > 0 ? 'pt-3' : ''}>
              <div className="mb-2 font-medium text-[var(--ui-text-primary)] text-[13px] leading-snug">
                {questions.length > 1 && <span className="text-[var(--ui-text-tertiary)] mr-1.5">{qIndex + 1}.</span>}
                {q.question}
              </div>

              <div className="flex flex-wrap gap-2 pt-1" role="group" aria-label={q.question}>
                {q.options.map((opt, optIndex) => {
                  const isSelected = chosen.includes(opt);

                  return (
                    <button
                      key={optIndex}
                      type="button"
                      disabled={disabled || submitted}
                      onClick={() => handleToggleOption(qIndex, opt, isMulti)}
                      className={`inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors duration-100 hover:transition-none ${
                        isSelected
                          ? 'border-[var(--ui-accent)] bg-[var(--ui-row-active)] text-[var(--ui-accent)]'
                          : 'border-[var(--ui-stroke-tertiary)] bg-[var(--ui-control-fill)] text-[var(--ui-text-secondary)] hover:border-[var(--ui-accent-border)] hover:bg-[var(--ui-control-hover)] hover:text-[var(--ui-text-primary)]'
                      } ${disabled || submitted ? 'opacity-70 cursor-default' : 'cursor-pointer active:scale-[0.98]'}`}
                    >
                      {isSelected && <Check size={12} className="shrink-0 text-[var(--ui-accent)]" />}
                      <span>{opt}</span>
                    </button>
                  );
                })}
              </div>

              {isMulti && !submitted && (
                <div className="mt-2.5 flex items-center justify-end">
                  <button
                    type="button"
                    disabled={disabled || submitted || chosen.length === 0}
                    onClick={() => handleMultiSubmit(qIndex)}
                    className="inline-flex items-center gap-1.5 rounded-lg bg-[var(--ui-accent)] px-3 py-1 text-xs font-medium text-[var(--ui-accent-fg)] transition-opacity hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed"
                  >
                    <Send size={12} />
                    <span>Submit selection</span>
                  </button>
                </div>
              )}
            </div>
          );
        })}
      </div>

      {submitted && (
        <div className="border-t border-[var(--ui-stroke-quaternary)] bg-[var(--ui-bg-chrome)] px-3.5 py-1.5 text-[11px] text-[var(--ui-green)] font-medium flex items-center gap-1.5">
          <Check size={12} />
          <span>Choice submitted to agent</span>
        </div>
      )}
    </div>
  );
};
