import { useEffect, useRef } from 'react';

const FOCUSABLE = [
  'button:not([disabled])',
  'input:not([disabled])',
  'textarea:not([disabled])',
  'select:not([disabled])',
  'a[href]',
  '[tabindex]:not([tabindex="-1"])',
].join(',');

/** Provides initial focus, focus containment, Escape handling, and focus restoration. */
export function useModalAccessibility<T extends HTMLElement>(
  open: boolean,
  onClose: () => void,
  initialFocusRef?: React.RefObject<HTMLElement | null>,
) {
  const dialogRef = useRef<T>(null);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    const previousFocus = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;

    const focusableElements = () => Array.from(
      dialogRef.current?.querySelectorAll<HTMLElement>(FOCUSABLE) || [],
    ).filter((element) => element.tabIndex >= 0 && element.getClientRects().length > 0);

    const frame = requestAnimationFrame(() => {
      const target = initialFocusRef?.current || focusableElements()[0] || dialogRef.current;
      target?.focus();
    });

    const onKeyDown = (event: KeyboardEvent) => {
      const dialogs = document.querySelectorAll<HTMLElement>('[role="dialog"]');
      if (dialogs[dialogs.length - 1] !== dialogRef.current) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        closeRef.current();
        return;
      }
      if (event.key !== 'Tab') return;
      const focusable = focusableElements();
      if (focusable.length === 0) {
        event.preventDefault();
        dialogRef.current?.focus();
        return;
      }
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (event.shiftKey && (active === first || !dialogRef.current?.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (active === last || !dialogRef.current?.contains(active))) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener('keydown', onKeyDown, true);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener('keydown', onKeyDown, true);
      if (previousFocus?.isConnected) previousFocus.focus();
    };
  }, [open, initialFocusRef]);

  return dialogRef;
}
