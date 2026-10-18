// The accent colour and the light/dark theme (Settings → Appearance), remembered per viewer. Every themed
// colour derives from one OKLCH hue and chroma (app.css), so each accent tints the whole interface evenly.

export interface Accent {
  id: string;
  /** Catalog key of its name. */
  name: string;
  hue: number;
  chroma: number;
}

const OCEAN: Accent = { id: 'ocean', name: 'Ocean', hue: 245, chroma: 0.15 };

export const ACCENTS: readonly Accent[] = [
  OCEAN,
  { id: 'teal', name: 'Teal', hue: 190, chroma: 0.12 },
  { id: 'violet', name: 'Violet', hue: 295, chroma: 0.15 },
  { id: 'rose', name: 'Rose', hue: 10, chroma: 0.15 },
  { id: 'amber', name: 'Amber', hue: 65, chroma: 0.14 },
  { id: 'graphite', name: 'Graphite', hue: 250, chroma: 0 },
];

export const DEFAULT_ACCENT = 'ocean';
const KEY = 'rd.accent';

export function accentById(id: string | null | undefined): Accent {
  return ACCENTS.find((a) => a.id === id) ?? OCEAN;
}

export function loadAccent(storage: Pick<Storage, 'getItem'> | undefined = globalThis.localStorage): Accent {
  try {
    return accentById(storage?.getItem(KEY));
  } catch {
    return accentById(DEFAULT_ACCENT);
  }
}

/** Sets the accent's variables on the document root (every window of the app calls this at start). */
export function applyAccent(
  accent: Accent,
  root: HTMLElement | undefined = globalThis.document?.documentElement,
) {
  if (!root) return;
  root.style.setProperty('--accent-hue', String(accent.hue));
  root.style.setProperty('--accent-chroma', String(accent.chroma));
  root.dataset.accent = accent.id;
}

export type Theme = 'system' | 'light' | 'dark';

export const THEMES: readonly { value: Theme; label: string }[] = [
  { value: 'system', label: 'System' },
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
];

const THEME_KEY = 'rd.theme';

export function loadTheme(storage: Pick<Storage, 'getItem'> | undefined = globalThis.localStorage): Theme {
  try {
    const value = storage?.getItem(THEME_KEY);
    return THEMES.some((t) => t.value === value) ? (value as Theme) : 'system';
  } catch {
    return 'system';
  }
}

const darkQuery = () =>
  typeof globalThis.matchMedia === 'function' ? globalThis.matchMedia('(prefers-color-scheme: dark)') : null;

/** Light or dark: "system" follows the OS. */
export function resolveTheme(theme: Theme, systemDark = darkQuery()?.matches ?? false): 'light' | 'dark' {
  return theme === 'system' ? (systemDark ? 'dark' : 'light') : theme;
}

/** Native window chrome (title bar, macOS materials) follows the theme too; null means the OS. */
export type NativeTheme = (theme: 'light' | 'dark' | null) => void;

let followSystem: (() => void) | undefined;

/**
 * Sets `data-theme` (app.css keys the dark tokens off it) and `color-scheme` on the document root, and
 * keeps "system" in step with the OS. Every window of the app calls this at start.
 */
export function applyTheme(
  theme: Theme,
  native?: NativeTheme,
  root: HTMLElement | undefined = globalThis.document?.documentElement,
) {
  if (!root) return;
  const set = () => {
    const resolved = resolveTheme(theme);
    root.dataset.theme = resolved;
    root.style.colorScheme = resolved;
  };
  set();
  const query = darkQuery();
  if (followSystem) query?.removeEventListener('change', followSystem);
  followSystem = theme === 'system' ? set : undefined;
  if (followSystem) query?.addEventListener('change', followSystem);
  native?.(theme === 'system' ? null : theme);
}

export function createAppearance(storage?: Pick<Storage, 'getItem' | 'setItem'>, native?: NativeTheme) {
  let accent = $state(loadAccent(storage));
  let theme = $state(loadTheme(storage));
  return {
    get accent() {
      return accent;
    },
    get theme() {
      return theme;
    },
    setTheme(value: Theme) {
      theme = value;
      applyTheme(theme, native);
      try {
        (storage ?? globalThis.localStorage)?.setItem(THEME_KEY, theme);
      } catch {
        // Storage unavailable: the theme just isn't remembered.
      }
    },
    setAccent(id: string) {
      accent = accentById(id);
      applyAccent(accent);
      try {
        (storage ?? globalThis.localStorage)?.setItem(KEY, accent.id);
      } catch {
        // Storage unavailable: the accent just isn't remembered.
      }
    },
  };
}

export type AppearanceStore = ReturnType<typeof createAppearance>;

export const appearance = createAppearance(undefined, (theme) => nativeTheme(theme));

/** Set by main.ts in the app (Tauri window API); a no-op in tests. */
let nativeTheme: NativeTheme = () => {};

export function setNativeTheme(fn: NativeTheme) {
  nativeTheme = fn;
}
