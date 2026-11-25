import { useSyncExternalStore } from 'react';
import {
  DEFAULT_THEME_ID,
  THEME_PRESETS,
  getThemePreset,
  isThemeId,
  type ThemeId,
  type ThemePreset,
} from './presets';

export const THEME_STORAGE_KEY = 'aether.theme';

const listeners = new Set<() => void>();

function getStoredTheme(): ThemeId {
  try {
    if (typeof window === 'undefined' || !window.localStorage) {
      return DEFAULT_THEME_ID;
    }
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    return isThemeId(stored) ? stored : DEFAULT_THEME_ID;
  } catch {
    return DEFAULT_THEME_ID;
  }
}

let currentTheme: ThemeId = getStoredTheme();

function notify(): void {
  for (const listener of listeners) {
    listener();
  }
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Applies dataset attribute, dark/light classes, and meta theme-color for the given preset. */
export function applyTheme(id: ThemeId): void {
  if (typeof document === 'undefined') return;
  const preset = getThemePreset(id);
  const root = document.documentElement;
  root.dataset.theme = preset.id;
  root.classList.toggle('dark', preset.mode === 'dark');
  root.classList.toggle('light', preset.mode === 'light');

  const metaThemeColor = document.querySelector('meta[name="theme-color"]');
  if (metaThemeColor) {
    metaThemeColor.setAttribute('content', preset.themeColor);
  }
}

/** Reads the persisted skin (falling back to Nous Dark) and paints the document root. */
export function initTheme(): ThemeId {
  currentTheme = getStoredTheme();
  applyTheme(currentTheme);
  return currentTheme;
}

/** Switches to a valid theme preset, persists to localStorage, and notifies subscribers. */
export function setTheme(id: ThemeId): void {
  if (!isThemeId(id)) return;
  currentTheme = id;
  try {
    if (typeof window !== 'undefined' && window.localStorage) {
      window.localStorage.setItem(THEME_STORAGE_KEY, id);
    }
  } catch {
    // Ignore storage quota/access errors in restricted environments.
  }
  applyTheme(id);
  notify();
}

/** Advances to the next built-in theme preset and persists it. */
export function cycleTheme(): ThemeId {
  const index = THEME_PRESETS.findIndex((preset) => preset.id === currentTheme);
  const next = THEME_PRESETS[(index + 1) % THEME_PRESETS.length];
  setTheme(next.id);
  return next.id;
}

export interface UseThemeResult {
  theme: ThemeId;
  preset: ThemePreset;
  presets: readonly ThemePreset[];
  setTheme: (id: ThemeId) => void;
  cycleTheme: () => ThemeId;
}

/** Subscribes a React component to the module theme store without requiring a context provider. */
export function useTheme(): UseThemeResult {
  const theme = useSyncExternalStore(
    subscribe,
    () => currentTheme,
    () => DEFAULT_THEME_ID,
  );
  return {
    theme,
    preset: getThemePreset(theme),
    presets: THEME_PRESETS,
    setTheme,
    cycleTheme,
  };
}

/** Resets theme store state cleanly between isolated unit tests. */
export function _resetThemeForTests(): void {
  currentTheme = DEFAULT_THEME_ID;
  try {
    if (typeof window !== 'undefined' && window.localStorage) {
      window.localStorage.removeItem(THEME_STORAGE_KEY);
    }
  } catch {
    // Ignore restricted storage in test sandboxes.
  }
  applyTheme(DEFAULT_THEME_ID);
  notify();
}
