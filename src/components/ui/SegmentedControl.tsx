import { useRef, type KeyboardEvent } from 'react';

export interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  title?: string;
  swatch?: string;
}

export interface SegmentedControlProps<T extends string> {
  options: readonly SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  ariaLabel: string;
  className?: string;
}

/** Accessible mutually-exclusive choice strip modeled on Hermes Desktop SegmentedControl. */
export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
  className = '',
}: SegmentedControlProps<T>) {
  const buttonRefs = useRef<Array<HTMLButtonElement | null>>([]);

  const handleKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key) || options.length === 0) return;
    event.preventDefault();
    const nextIndex =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? options.length - 1
          : (index + (event.key === 'ArrowRight' ? 1 : -1) + options.length) % options.length;
    const target = options[nextIndex];
    onChange(target.value);
    buttonRefs.current[nextIndex]?.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      className={`inline-flex flex-wrap items-center gap-0.5 rounded-[7px] bg-[var(--ui-control-fill)] p-0.5 ${className}`.trim()}
    >
      {options.map((option, index) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            ref={(el) => {
              buttonRefs.current[index] = el;
            }}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            title={option.title}
            onClick={() => onChange(option.value)}
            onKeyDown={(event) => handleKeyDown(event, index)}
            className={`inline-flex items-center gap-1.5 rounded-[5px] px-2.5 py-1 text-[11.5px] transition-colors duration-100 hover:transition-none ${
              selected
                ? 'bg-[var(--ui-bg-elevated)] font-medium text-[var(--ui-text-primary)] shadow-sm'
                : 'bg-transparent text-[var(--ui-text-secondary)] hover:text-[var(--ui-text-primary)]'
            }`}
          >
            {option.swatch && (
              <span
                aria-hidden="true"
                className="h-2 w-2 shrink-0 rounded-full border border-[var(--ui-stroke-secondary)]"
                style={{ backgroundColor: option.swatch }}
              />
            )}
            <span>{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}
