/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        background: 'var(--ui-bg-chrome)',
        sidebar: 'var(--ui-bg-sidebar)',
        surface: 'var(--ui-bg-elevated)',
        editor: 'var(--ui-bg-editor)',
        widget: 'var(--ui-widget-bg)',
        surfaceBorder: 'var(--ui-stroke-secondary)',
        hairline: 'var(--ui-stroke-tertiary)',
        surfaceHover: 'var(--ui-row-hover)',
        accent: 'var(--ui-accent)',
        accentHover: 'var(--ui-accent-hover)',
        ink: {
          primary: 'var(--ui-text-primary)',
          secondary: 'var(--ui-text-secondary)',
          tertiary: 'var(--ui-text-tertiary)',
          muted: 'var(--ui-text-quaternary)',
        },
      },
      boxShadow: {
        nous: 'var(--shadow-nous)',
        lg: 'var(--shadow-nous)',
        composer: 'var(--shadow-composer)',
      },
      fontFamily: {
        mono: ['Fira Code', 'JetBrains Mono', 'Consolas', 'monospace'],
      }
    },
  },
  plugins: [],
}
