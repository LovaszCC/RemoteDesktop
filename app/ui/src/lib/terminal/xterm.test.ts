import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ShortcutDto } from '../types';

// xterm needs a real canvas; these fakes record how the wrapper drives it.
const created: FakeTerminal[] = [];
const webgls: FakeWebgl[] = [];

class FakeTerminal {
  options: Record<string, unknown>;
  cols = 80;
  rows = 24;
  unicode = { activeVersion: '6' };
  addons: unknown[] = [];
  keyHandler: ((e: KeyboardEvent) => boolean) | null = null;
  data: ((d: string) => void) | null = null;
  binary: ((d: string) => void) | null = null;
  resize: ((s: { cols: number; rows: number }) => void) | null = null;
  write = vi.fn();
  paste = vi.fn();
  focus = vi.fn();
  dispose = vi.fn();
  clearSelection = vi.fn();
  open = vi.fn();
  getSelection = vi.fn(() => 'selected');
  constructor(options: Record<string, unknown>) {
    this.options = options;
    created.push(this);
  }
  loadAddon(addon: unknown) {
    this.addons.push(addon);
  }
  onData(fn: (d: string) => void) {
    this.data = fn;
  }
  onBinary(fn: (d: string) => void) {
    this.binary = fn;
  }
  onResize(fn: (s: { cols: number; rows: number }) => void) {
    this.resize = fn;
  }
  attachCustomKeyEventHandler(fn: (e: KeyboardEvent) => boolean) {
    this.keyHandler = fn;
  }
}

class FakeWebgl {
  lost: (() => void) | null = null;
  dispose = vi.fn();
  constructor() {
    webgls.push(this);
  }
  onContextLoss(fn: () => void) {
    this.lost = fn;
  }
}

vi.mock('@xterm/xterm', () => ({ Terminal: FakeTerminal }));
vi.mock('@xterm/xterm/css/xterm.css', () => ({}));
vi.mock('@xterm/addon-webgl', () => ({ WebglAddon: FakeWebgl }));
vi.mock('@xterm/addon-fit', () => ({ FitAddon: class {} }));
vi.mock('@xterm/addon-unicode11', () => ({ Unicode11Addon: class {} }));
vi.mock('@xterm/addon-web-links', () => ({
  WebLinksAddon: class {
    constructor(public handler: (e: MouseEvent, uri: string) => void) {}
  },
}));

const { createXtermTerminal, LOCAL_CLEAR_SEQUENCE, RESIZE_DEBOUNCE, SCROLLBACK } = await import('./xterm');

const shortcuts: ShortcutDto[] = [
  {
    command: 'clearTerminal',
    group: 'connection',
    shortcut: { key: 'k', modifiers: 1 },
    display: '⌘K',
    defaultDisplay: '⌘K',
    customized: false,
  },
];

function setup(recording = false) {
  const options = {
    onInput: vi.fn(),
    onResize: vi.fn(),
    keyContext: () => ({ platform: 'linux' as const, shortcuts, recording }),
    cancelRecording: vi.fn(),
    copy: vi.fn(),
    paste: vi.fn(async () => 'clip'),
    openLink: vi.fn(),
    initialSize: { cols: 100, rows: 30 },
  };
  const handle = createXtermTerminal(options);
  const term = created.at(-1)!;
  return { options, handle, term };
}

const key = (init: KeyboardEventInit, type = 'keydown') =>
  new KeyboardEvent(type, { cancelable: true, code: '', ...init });

afterEach(() => {
  created.length = 0;
  webgls.length = 0;
  vi.useRealTimers();
});

describe('xterm terminal', () => {
  it('configures scrollback, Option as typing and Unicode 11', () => {
    const { term } = setup();
    expect(term.options).toMatchObject({
      scrollback: SCROLLBACK,
      macOptionIsMeta: false,
      cols: 100,
      rows: 30,
    });
    expect(term.unicode.activeVersion).toBe('11');
  });

  it('opens on first attach with WebGL and falls back to the DOM renderer on context loss', () => {
    const { handle, term } = setup();
    const container = document.createElement('div');
    handle.attach(container);
    handle.attach(container);
    expect(term.open).toHaveBeenCalledTimes(1);
    expect(container.querySelector('[data-terminal]')).not.toBeNull();
    expect(webgls).toHaveLength(1);
    webgls[0]!.lost!();
    expect(webgls[0]!.dispose).toHaveBeenCalled();
    handle.detach();
    expect(container.children).toHaveLength(0);
  });

  it('passes typed text and binary input on', () => {
    const { options, term } = setup();
    term.data!('ls\r');
    term.binary!('\x1b\xff');
    expect(options.onInput).toHaveBeenCalledWith('ls\r');
    expect(options.onInput).toHaveBeenCalledWith([27, 255]);
  });

  it('reports the settled size after resizes', () => {
    vi.useFakeTimers();
    const { options, term } = setup();
    term.resize!({ cols: 90, rows: 20 });
    term.resize!({ cols: 120, rows: 40 });
    vi.advanceTimersByTime(RESIZE_DEBOUNCE);
    expect(options.onResize).toHaveBeenCalledTimes(1);
    expect(options.onResize).toHaveBeenCalledWith({ cols: 120, rows: 40 });
  });

  it('lets app shortcuts through to the window and keeps plain keys', () => {
    const { term } = setup();
    const clear = key({ key: 'k', code: 'KeyK', metaKey: true });
    expect(term.keyHandler!(clear)).toBe(false);
    expect(clear.defaultPrevented).toBe(false);
    expect(term.keyHandler!(key({ key: 'a', code: 'KeyA' }))).toBe(true);
  });

  it('sends ⌥← as ESC b, once per key press', () => {
    const { options, term } = setup();
    const down = key({ key: 'ArrowLeft', altKey: true });
    expect(term.keyHandler!(down)).toBe(false);
    expect(down.defaultPrevented).toBe(true);
    term.keyHandler!(key({ key: 'ArrowLeft', altKey: true }, 'keyup'));
    expect(options.onInput).toHaveBeenCalledTimes(1);
    expect(options.onInput).toHaveBeenCalledWith('\x1bb');
  });

  it('copies the selection and pastes the clipboard with Ctrl+Shift+C/V', async () => {
    const { options, term } = setup();
    term.keyHandler!(key({ key: 'C', code: 'KeyC', ctrlKey: true, shiftKey: true }));
    expect(options.copy).toHaveBeenCalledWith('selected');
    term.keyHandler!(key({ key: 'V', code: 'KeyV', ctrlKey: true, shiftKey: true }));
    await Promise.resolve();
    await Promise.resolve();
    expect(term.paste).toHaveBeenCalledWith('clip');
  });

  it('cancels a recording on Esc', () => {
    const { options, term } = setup(true);
    term.keyHandler!(key({ key: 'Escape', code: 'Escape' }));
    expect(options.cancelRecording).toHaveBeenCalled();
  });

  it('clears screen and scrollback locally', () => {
    const { handle, term } = setup();
    handle.clearLocal();
    expect(term.write).toHaveBeenCalledWith(LOCAL_CLEAR_SEQUENCE);
  });

  it('opens clicked links through the app', () => {
    const { options, term } = setup();
    const links = term.addons.find((a) => (a as { handler?: unknown }).handler) as {
      handler: (e: MouseEvent, uri: string) => void;
    };
    links.handler(new MouseEvent('click', { cancelable: true }), 'https://example.com');
    expect(options.openLink).toHaveBeenCalledWith('https://example.com');
  });

  it('disposes everything', () => {
    const { handle, term } = setup();
    handle.attach(document.createElement('div'));
    handle.dispose();
    expect(term.dispose).toHaveBeenCalled();
    expect(webgls[0]!.dispose).toHaveBeenCalled();
  });
});
