// One embedded terminal: an xterm.js instance that lives as long as its session (Swift `TerminalSession`
// owns its view), so output and scrollback survive tab switches and locking. Views attach its element to
// a container and detach it again; the instance is only disposed when the tab closes.
//
// Renderer: WebGL, falling back to xterm's DOM renderer when the context is lost or WebGL is missing.

import { FitAddon } from '@xterm/addon-fit';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebglAddon } from '@xterm/addon-webgl';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { Terminal, type ITheme } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';
import { TERMINAL_COLOR_KEYS, TERMINAL_DEFAULTS, terminalColorVar } from './colors';
import { terminalKeyAction, type TerminalKeyAction, type TerminalKeyContext } from './keys';

/** Wipes the screen and the scrollback (`rd_ssh::analyzer::LOCAL_CLEAR_SEQUENCE`). */
export const LOCAL_CLEAR_SEQUENCE = '\x1b[H\x1b[2J\x1b[3J';
export const SCROLLBACK = 5_000;
/** A terminal hidden this long gives its WebGL context back (atlas, buffers); it is made again when shown. */
export const WEBGL_IDLE_MS = 30_000;
/** How long resizes settle before the PTY hears about them, in ms. */
export const RESIZE_DEBOUNCE = 80;

export interface TerminalSize {
  cols: number;
  rows: number;
}

/** What a session needs from its terminal; tests pass fakes. */
export interface TerminalHandle {
  /** `done` runs once xterm parsed the data (output flow control). */
  write(data: Uint8Array | string, done?: () => void): void;
  /** Screen and scrollback, locally. */
  clearLocal(): void;
  attach(container: HTMLElement): void;
  /** Takes the element out of `container` (any container when omitted); a no-op once it moved elsewhere. */
  detach(container?: HTMLElement): void;
  /** Fits the terminal to its container (no-op while hidden). */
  fit(): void;
  /** The terminal is on screen or not: hidden for [`WEBGL_IDLE_MS`] it drops its WebGL renderer. */
  setVisible?(visible: boolean): void;
  size(): TerminalSize;
  focus(): void;
  dispose(): void;
}

export interface TerminalOptions {
  /** Typed text (`onData`) or raw bytes (`onBinary`). */
  onInput(data: string | number[]): void;
  /** The settled size after a resize. */
  onResize(size: TerminalSize): void;
  keyContext(): TerminalKeyContext;
  cancelRecording(): void;
  copy(text: string): void;
  /** The clipboard text, or `null`. */
  paste(): Promise<string | null>;
  openLink(url: string): void;
  initialSize?: TerminalSize;
}

export type TerminalFactory = (options: TerminalOptions) => TerminalHandle;

function cssToken(name: string, fallback: string): string {
  if (typeof document === 'undefined') return fallback;
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value || fallback;
}

/** The terminal colours from the `--terminal-*` variables (`colors.ts`); Catppuccin Mocha when unset. */
export function themeFromTokens(): ITheme {
  const theme: ITheme = {};
  for (const key of TERMINAL_COLOR_KEYS)
    theme[key] = cssToken(terminalColorVar(key), TERMINAL_DEFAULTS.dark[key]);
  theme.cursorAccent = theme.background;
  return theme;
}

/** Performs a non-terminal key action; returns whether the event was consumed. */
function perform(action: TerminalKeyAction, term: Terminal, options: TerminalOptions): void {
  switch (action.kind) {
    case 'send':
      options.onInput(action.data);
      return;
    case 'copy': {
      const text = term.getSelection();
      if (text) options.copy(text);
      return;
    }
    case 'paste':
      void options.paste().then((text) => {
        if (text) term.paste(text);
      });
      return;
    case 'cancelRecording':
      options.cancelRecording();
      return;
    default:
      return;
  }
}

export const createXtermTerminal: TerminalFactory = (options) => {
  const element = document.createElement('div');
  element.className = 'terminal-host';
  element.dataset.terminal = '';
  const term = new Terminal({
    allowProposedApi: true,
    scrollback: SCROLLBACK,
    fontFamily: cssToken('--font-mono', 'monospace'),
    fontSize: 13,
    cursorBlink: true,
    // Option types the layout's characters (@, |, [ …), as in the Swift app; ⌥←/→ are handled below.
    macOptionIsMeta: false,
    theme: themeFromTokens(),
    cols: options.initialSize?.cols ?? 80,
    rows: options.initialSize?.rows ?? 24,
  });
  const fitAddon = new FitAddon();
  term.loadAddon(fitAddon);
  term.loadAddon(new Unicode11Addon());
  term.unicode.activeVersion = '11';
  term.loadAddon(
    new WebLinksAddon((event, uri) => {
      event.preventDefault();
      options.openLink(uri);
    }),
  );

  term.onData((data) => options.onInput(data));
  term.onBinary((data) => options.onInput(Array.from(data, (c) => c.charCodeAt(0) & 0xff)));
  term.attachCustomKeyEventHandler((event) => {
    const action = terminalKeyAction(event, options.keyContext());
    if (action.kind === 'terminal') return true;
    // App shortcuts and the native ⌘C/⌘V: xterm leaves the event alone, so it reaches the window.
    if (action.kind === 'app' || action.kind === 'native') return false;
    event.preventDefault();
    if (event.type === 'keydown') perform(action, term, options);
    return false;
  });

  let resizeTimer: ReturnType<typeof setTimeout> | undefined;
  term.onResize(({ cols, rows }) => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => options.onResize({ cols, rows }), RESIZE_DEBOUNCE);
  });

  // The theme switches by data-theme on the root (applyTheme), from the OS or Settings; the colours by
  // data-terminal-colors (applyTerminalColors).
  const retheme = new MutationObserver(() => {
    term.options.theme = themeFromTokens();
  });
  retheme.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ['data-theme', 'data-accent', 'data-terminal-colors'],
  });

  let opened = false;
  let webgl: WebglAddon | null = null;
  let idleTimer: ReturnType<typeof setTimeout> | undefined;

  function loadWebgl() {
    try {
      const addon = new WebglAddon();
      addon.onContextLoss(() => {
        // Back to the DOM renderer.
        addon.dispose();
        if (webgl === addon) webgl = null;
      });
      term.loadAddon(addon);
      webgl = addon;
    } catch {
      webgl = null;
    }
  }

  return {
    write(data, done) {
      term.write(data, done);
    },
    clearLocal() {
      term.clearSelection();
      term.write(LOCAL_CLEAR_SEQUENCE);
    },
    attach(container) {
      if (element.parentElement !== container) container.appendChild(element);
      if (!opened) {
        term.open(element);
        opened = true;
        loadWebgl();
      }
      this.fit();
    },
    detach(container) {
      if (container && element.parentElement !== container) return;
      element.remove();
    },
    fit() {
      if (!opened || !element.isConnected || element.clientWidth === 0 || element.clientHeight === 0) return;
      try {
        fitAddon.fit();
      } catch {
        // Not measurable yet (hidden); the next resize fits.
      }
    },
    size() {
      return { cols: term.cols, rows: term.rows };
    },
    setVisible(visible) {
      clearTimeout(idleTimer);
      if (visible) {
        if (opened && !webgl) loadWebgl();
        return;
      }
      idleTimer = setTimeout(() => {
        webgl?.dispose();
        webgl = null;
      }, WEBGL_IDLE_MS);
    },
    focus() {
      term.focus();
    },
    dispose() {
      clearTimeout(resizeTimer);
      clearTimeout(idleTimer);
      retheme.disconnect();
      webgl?.dispose();
      term.dispose();
      element.remove();
    },
  };
};
