import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  DEFAULT_THEME_ID,
  THEME_PRESETS,
  THEME_STORAGE_KEY,
  _resetThemeForTests,
  cycleTheme,
  initTheme,
  setTheme,
  useTheme,
} from './index';

function ThemeProbe() {
  const { theme, preset } = useTheme();
  return (
    <div>
      <span data-testid="theme-id">{theme}</span>
      <span data-testid="theme-mode">{preset.mode}</span>
    </div>
  );
}

describe('themeStore', () => {
  beforeEach(() => {
    if (!document.querySelector('meta[name="theme-color"]')) {
      const meta = document.createElement('meta');
      meta.setAttribute('name', 'theme-color');
      meta.setAttribute('content', '#0d1117');
      document.head.appendChild(meta);
    }
    _resetThemeForTests();
  });

  afterEach(() => {
    cleanup();
    _resetThemeForTests();
  });

  it('defaults to nous-dark and applies root attributes', () => {
    const resolved = initTheme();
    expect(resolved).toBe(DEFAULT_THEME_ID);
    expect(document.documentElement.dataset.theme).toBe('nous-dark');
    expect(document.documentElement.classList.contains('dark')).toBe(true);
    expect(document.documentElement.classList.contains('light')).toBe(false);
    expect(document.querySelector('meta[name="theme-color"]')?.getAttribute('content')).toBe('#0d1117');
  });

  it('persists light theme selection and toggles classes and meta theme-color', () => {
    setTheme('nous-light');
    expect(window.localStorage.getItem(THEME_STORAGE_KEY)).toBe('nous-light');
    expect(document.documentElement.dataset.theme).toBe('nous-light');
    expect(document.documentElement.classList.contains('light')).toBe(true);
    expect(document.documentElement.classList.contains('dark')).toBe(false);
    expect(document.querySelector('meta[name="theme-color"]')?.getAttribute('content')).toBe('#ffffff');
  });

  it('falls back to nous-dark when stored theme value is invalid', () => {
    window.localStorage.setItem(THEME_STORAGE_KEY, 'not-a-real-skin');
    const resolved = initTheme();
    expect(resolved).toBe('nous-dark');
    expect(document.documentElement.dataset.theme).toBe('nous-dark');
  });

  it('cycles through all presets and wraps back to the first', () => {
    initTheme();
    const seen = [THEME_PRESETS[0].id];
    for (let i = 1; i < THEME_PRESETS.length; i += 1) {
      seen.push(cycleTheme());
    }
    expect(seen).toEqual(THEME_PRESETS.map((preset) => preset.id));
    expect(cycleTheme()).toBe(THEME_PRESETS[0].id);
  });

  it('updates subscribed React components without requiring a provider', () => {
    render(<ThemeProbe />);
    expect(screen.getByTestId('theme-id').textContent).toBe('nous-dark');
    expect(screen.getByTestId('theme-mode').textContent).toBe('dark');

    act(() => {
      setTheme('ember');
    });
    expect(screen.getByTestId('theme-id').textContent).toBe('ember');
    expect(screen.getByTestId('theme-mode').textContent).toBe('dark');

    act(() => {
      setTheme('nous-light');
    });
    expect(screen.getByTestId('theme-id').textContent).toBe('nous-light');
    expect(screen.getByTestId('theme-mode').textContent).toBe('light');
  });
});
