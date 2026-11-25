export type ThemeId = 'nous-dark' | 'antigravity' | 'nous-light' | 'catppuccin' | 'ember';

export const DEFAULT_THEME_ID: ThemeId = 'nous-dark';

export interface ThemePreset {
  id: ThemeId;
  label: string;
  shortLabel: string;
  description: string;
  mode: 'dark' | 'light';
  themeColor: string;
  swatches: {
    bg: string;
    sidebar: string;
    accent: string;
    tool: string;
  };
}

export const THEME_PRESETS: readonly ThemePreset[] = [
  {
    id: 'nous-dark',
    label: 'Nous Dark',
    shortLabel: 'Nous',
    description: 'Hermes canonical dark surface with Nous blue accent',
    mode: 'dark',
    themeColor: '#0d1117',
    swatches: { bg: '#0d1117', sidebar: '#010409', accent: '#4a84fe', tool: '#4a84fe' },
  },
  {
    id: 'antigravity',
    label: 'Antigravity Graphite',
    shortLabel: 'Graphite',
    description: 'Neutral calm charcoal with soft blue highlights',
    mode: 'dark',
    themeColor: '#131417',
    swatches: { bg: '#131417', sidebar: '#0e0f12', accent: '#7aa2ff', tool: '#7aa2ff' },
  },
  {
    id: 'nous-light',
    label: 'Nous Light',
    shortLabel: 'Light',
    description: 'Crisp light workspace with high-contrast ink and Nous blue',
    mode: 'light',
    themeColor: '#ffffff',
    swatches: { bg: '#ffffff', sidebar: '#f6f8fa', accent: '#0053fd', tool: '#0053fd' },
  },
  {
    id: 'catppuccin',
    label: 'Catppuccin Mocha',
    shortLabel: 'Mocha',
    description: 'Warm slate pastels with mauve tool markers',
    mode: 'dark',
    themeColor: '#1e1e2e',
    swatches: { bg: '#1e1e2e', sidebar: '#181825', accent: '#89b4fa', tool: '#cba6f7' },
  },
  {
    id: 'ember',
    label: 'Hermes Ember',
    shortLabel: 'Ember',
    description: 'Warm obsidian charcoal with gold tool accents',
    mode: 'dark',
    themeColor: '#141110',
    swatches: { bg: '#141110', sidebar: '#0d0a09', accent: '#f0a43b', tool: '#f5b942' },
  },
];

const THEME_IDS = new Set<string>(THEME_PRESETS.map((preset) => preset.id));

export function isThemeId(value: unknown): value is ThemeId {
  return typeof value === 'string' && THEME_IDS.has(value);
}

export function getThemePreset(id: ThemeId): ThemePreset {
  return THEME_PRESETS.find((preset) => preset.id === id) ?? THEME_PRESETS[0];
}
