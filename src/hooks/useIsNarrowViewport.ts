import { useEffect, useState } from 'react';

const NARROW_QUERY = '(max-width: 767px)';

function matchesNarrow(): boolean {
  if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return false;
  return window.matchMedia(NARROW_QUERY).matches;
}

/** Tracks whether the viewport is narrow enough to require the overlay sidebar layout. */
export function useIsNarrowViewport(): boolean {
  const [narrow, setNarrow] = useState(matchesNarrow);

  useEffect(() => {
    if (typeof window === 'undefined' || typeof window.matchMedia !== 'function') return;
    const list = window.matchMedia(NARROW_QUERY);
    const onChange = (event: MediaQueryListEvent) => setNarrow(event.matches);
    setNarrow(list.matches);
    list.addEventListener('change', onChange);
    return () => list.removeEventListener('change', onChange);
  }, []);

  return narrow;
}
