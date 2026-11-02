// The terminal's colours (Settings → Terminal colors): one set for the app's light mode and one for dark,
// Catppuccin Latte and Mocha unless the user changed a colour. Only the changes are stored, so the
// defaults can improve later. They reach xterm as `--terminal-*` variables (a style tag keyed off
// `data-theme`, like app.css), which `xterm.ts` reads whenever the theme or the colours change.

export const TERMINAL_COLOR_KEYS = [
  'background',
  'foreground',
  'cursor',
  'selectionBackground',
  'black',
  'red',
  'green',
  'yellow',
  'blue',
  'magenta',
  'cyan',
  'white',
  'brightBlack',
  'brightRed',
  'brightGreen',
  'brightYellow',
  'brightBlue',
  'brightMagenta',
  'brightCyan',
  'brightWhite',
] as const;

export type TerminalColorKey = (typeof TERMINAL_COLOR_KEYS)[number];
export type TerminalMode = 'light' | 'dark';
export type TerminalPalette = Record<TerminalColorKey, string>;
/** The user's changes per mode. */
export type TerminalColors = Record<TerminalMode, Partial<TerminalPalette>>;

/** Catppuccin Mocha (https://catppuccin.com/palette). */
const MOCHA: TerminalPalette = {
  background: '#1e1e2e',
  foreground: '#cdd6f4',
  cursor: '#f5e0dc',
  selectionBackground: '#9399b240',
  black: '#45475a',
  red: '#f38ba8',
  green: '#a6e3a1',
  yellow: '#f9e2af',
  blue: '#89b4fa',
  magenta: '#f5c2e7',
  cyan: '#94e2d5',
  white: '#bac2de',
  brightBlack: '#585b70',
  brightRed: '#f38ba8',
  brightGreen: '#a6e3a1',
  brightYellow: '#f9e2af',
  brightBlue: '#89b4fa',
  brightMagenta: '#f5c2e7',
  brightCyan: '#94e2d5',
  brightWhite: '#a6adc8',
};

/** Catppuccin Latte. */
const LATTE: TerminalPalette = {
  background: '#eff1f5',
  foreground: '#4c4f69',
  cursor: '#dc8a78',
  selectionBackground: '#7c7f9340',
  black: '#5c5f77',
  red: '#d20f39',
  green: '#40a02b',
  yellow: '#df8e1d',
  blue: '#1e66f5',
  magenta: '#ea76cb',
  cyan: '#179299',
  white: '#acb0be',
  brightBlack: '#6c6f85',
  brightRed: '#d20f39',
  brightGreen: '#40a02b',
  brightYellow: '#df8e1d',
  brightBlue: '#1e66f5',
  brightMagenta: '#ea76cb',
  brightCyan: '#179299',
  brightWhite: '#bcc0cc',
};

export const TERMINAL_DEFAULTS: Record<TerminalMode, TerminalPalette> = { light: LATTE, dark: MOCHA };

export const TERMINAL_COLORS_KEY = 'rd.terminalColors';
const STYLE_ID = 'terminal-colors';

/** `#rrggbb` or `#rrggbbaa`. */
export function isTerminalColor(value: unknown): value is string {
  return typeof value === 'string' && /^#[0-9a-f]{6}([0-9a-f]{2})?$/i.test(value);
}

export function emptyTerminalColors(): TerminalColors {
  return { light: {}, dark: {} };
}

/** Stored changes; bad JSON, unknown keys and invalid colours are dropped. */
export function parseTerminalColors(json: string | null | undefined): TerminalColors {
  const colors = emptyTerminalColors();
  let parsed: unknown;
  try {
    parsed = JSON.parse(json ?? 'null');
  } catch {
    return colors;
  }
  if (typeof parsed !== 'object' || parsed === null) return colors;
  for (const mode of ['light', 'dark'] as const) {
    const set: unknown = (parsed as Record<string, unknown>)[mode];
    if (typeof set !== 'object' || set === null) continue;
    for (const key of TERMINAL_COLOR_KEYS) {
      const value: unknown = (set as Record<string, unknown>)[key];
      if (isTerminalColor(value)) colors[mode][key] = value;
    }
  }
  return colors;
}

export function loadTerminalColors(
  storage: Pick<Storage, 'getItem'> | undefined = globalThis.localStorage,
): TerminalColors {
  try {
    return parseTerminalColors(storage?.getItem(TERMINAL_COLORS_KEY));
  } catch {
    return emptyTerminalColors();
  }
}

/** A mode's full palette: the defaults with the user's changes. */
export function terminalPalette(mode: TerminalMode, colors: TerminalColors): TerminalPalette {
  return { ...TERMINAL_DEFAULTS[mode], ...colors[mode] };
}

/** `selectionBackground` → `--terminal-selection-background`. */
export function terminalColorVar(key: TerminalColorKey): string {
  return `--terminal-${key.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`;
}

/** The variables of both modes: light on `:root`, dark on `:root[data-theme='dark']`. */
export function terminalColorsCss(colors: TerminalColors): string {
  const block = (mode: TerminalMode) => {
    const palette = terminalPalette(mode, colors);
    return TERMINAL_COLOR_KEYS.map((key) => `${terminalColorVar(key)}:${palette[key]};`).join('');
  };
  return `:root{${block('light')}}:root[data-theme='dark']{${block('dark')}}`;
}

/** Puts the colours into the document; open terminals re-theme (`data-terminal-colors` changes). */
export function applyTerminalColors(colors: TerminalColors, doc: Document | undefined = globalThis.document) {
  if (!doc) return;
  let style = doc.getElementById(STYLE_ID);
  if (!style) {
    style = doc.createElement('style');
    style.id = STYLE_ID;
    doc.head.appendChild(style);
  }
  style.textContent = terminalColorsCss(colors);
  const root = doc.documentElement;
  root.dataset.terminalColors = String(Number(root.dataset.terminalColors ?? '0') + 1);
}
