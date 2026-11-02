import { afterEach, describe, expect, it } from 'vitest';
import {
  TERMINAL_COLOR_KEYS,
  TERMINAL_COLORS_KEY,
  TERMINAL_DEFAULTS,
  applyTerminalColors,
  isTerminalColor,
  loadTerminalColors,
  parseTerminalColors,
  terminalColorVar,
  terminalColorsCss,
  terminalPalette,
} from './colors';

afterEach(() => {
  document.getElementById('terminal-colors')?.remove();
  delete document.documentElement.dataset.terminalColors;
});

describe('terminal colours', () => {
  it('defaults to Catppuccin Latte and Mocha with every colour valid', () => {
    for (const mode of ['light', 'dark'] as const) {
      expect(Object.keys(TERMINAL_DEFAULTS[mode]).sort()).toEqual([...TERMINAL_COLOR_KEYS].sort());
      expect(Object.values(TERMINAL_DEFAULTS[mode]).every(isTerminalColor)).toBe(true);
    }
    expect(TERMINAL_DEFAULTS.dark.background).toBe('#1e1e2e');
    expect(TERMINAL_DEFAULTS.dark.foreground).toBe('#cdd6f4');
    expect(TERMINAL_DEFAULTS.light.background).toBe('#eff1f5');
  });

  it('accepts #rrggbb and #rrggbbaa only', () => {
    expect(isTerminalColor('#A1b2c3')).toBe(true);
    expect(isTerminalColor('#a1b2c3d4')).toBe(true);
    for (const bad of ['#abc', 'red', 'a1b2c3', '#a1b2c3d', '#gggggg', 1, null]) {
      expect(isTerminalColor(bad)).toBe(false);
    }
  });

  it('parses stored changes, dropping bad JSON, unknown keys and invalid colours', () => {
    expect(parseTerminalColors('{oops')).toEqual({ light: {}, dark: {} });
    expect(parseTerminalColors(null)).toEqual({ light: {}, dark: {} });
    expect(parseTerminalColors('[1]')).toEqual({ light: {}, dark: {} });
    const json = JSON.stringify({
      dark: { background: '#000000', red: 'red', nope: '#111111' },
      light: 'x',
      other: {},
    });
    expect(parseTerminalColors(json)).toEqual({ light: {}, dark: { background: '#000000' } });
  });

  it('loads from storage, empty when storage fails', () => {
    const storage = {
      getItem: (key: string) => (key === TERMINAL_COLORS_KEY ? '{"light":{"red":"#ff0000"}}' : null),
    };
    expect(loadTerminalColors(storage)).toEqual({ light: { red: '#ff0000' }, dark: {} });
    const broken = {
      getItem: () => {
        throw new Error('denied');
      },
    };
    expect(loadTerminalColors(broken)).toEqual({ light: {}, dark: {} });
  });

  it('merges changes over the defaults', () => {
    const palette = terminalPalette('dark', { light: {}, dark: { red: '#ff0000' } });
    expect(palette.red).toBe('#ff0000');
    expect(palette.green).toBe(TERMINAL_DEFAULTS.dark.green);
  });

  it('writes a variable per colour for both modes', () => {
    expect(terminalColorVar('selectionBackground')).toBe('--terminal-selection-background');
    expect(terminalColorVar('brightWhite')).toBe('--terminal-bright-white');
    const css = terminalColorsCss({ light: {}, dark: { background: '#000000' } });
    expect(css).toContain(":root[data-theme='dark']{--terminal-background:#000000;");
    expect(css.startsWith(`:root{--terminal-background:${TERMINAL_DEFAULTS.light.background};`)).toBe(true);
    expect(css.match(/--terminal-/g)).toHaveLength(TERMINAL_COLOR_KEYS.length * 2);
  });

  it('keeps one style tag up to date and tells terminals', () => {
    applyTerminalColors({ light: {}, dark: {} });
    applyTerminalColors({ light: {}, dark: { red: '#ff0000' } });
    const styles = document.querySelectorAll('#terminal-colors');
    expect(styles).toHaveLength(1);
    expect(styles[0]!.textContent).toContain('--terminal-red:#ff0000;');
    expect(document.documentElement.dataset.terminalColors).toBe('2');
  });
});
