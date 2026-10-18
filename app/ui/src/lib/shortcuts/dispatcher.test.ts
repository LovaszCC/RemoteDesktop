import { afterEach, describe, expect, it } from 'vitest';
import type { ShortcutDto } from '../types';
import {
  commandFor,
  createDispatcher,
  isRdpTarget,
  isTerminalTarget,
  isTypingTarget,
  rdpCommand,
  type AppCommand,
} from './dispatcher';

const rows: ShortcutDto[] = [
  {
    command: 'newRDP',
    group: 'file',
    shortcut: { key: 'n', modifiers: 1 },
    display: '⌘N',
    defaultDisplay: '⌘N',
    customized: false,
  },
  {
    command: 'closeTab',
    group: 'connection',
    shortcut: { key: 'w', modifiers: 1 },
    display: '⌘W',
    defaultDisplay: '⌘W',
    customized: false,
  },
  {
    command: 'clearTerminal',
    group: 'connection',
    shortcut: { key: 'k', modifiers: 1 },
    display: '⌘K',
    defaultDisplay: '⌘K',
    customized: false,
  },
  {
    command: 'splitTerminal',
    group: 'connection',
    shortcut: { key: 'd', modifiers: 1 },
    display: '⌘D',
    defaultDisplay: '⌘D',
    customized: false,
  },
  {
    command: 'lock',
    group: 'connection',
    shortcut: null,
    display: null,
    defaultDisplay: '⌃⌘L',
    customized: true,
  },
];

function setup(options: { recording?: boolean; dev?: boolean } = {}) {
  const ran: AppCommand[] = [];
  let time = 1000;
  const dispatcher = createDispatcher({
    shortcuts: () => rows,
    platform: () => 'macos',
    isRecording: () => options.recording ?? false,
    run: (c) => ran.push(c),
    dev: options.dev ?? false,
    now: () => time,
  });
  return { dispatcher, ran, advance: (ms: number) => (time += ms) };
}

function keydown(target: EventTarget, init: KeyboardEventInit) {
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('commandFor', () => {
  it('finds the effective shortcut, Settings and the dev test tab', () => {
    expect(commandFor({ key: 'n', modifiers: 1 }, rows, 'macos')).toBe('newRDP');
    expect(commandFor({ key: 'n', modifiers: 3 }, rows, 'macos')).toBeNull();
    expect(commandFor({ key: ',', modifiers: 1 }, rows, 'macos')).toBe('openSettings');
    expect(commandFor({ key: ',', modifiers: 8 }, rows, 'windows')).toBe('openSettings');
    expect(commandFor({ key: ',', modifiers: 8 }, rows, 'macos')).toBeNull();
    expect(commandFor({ key: 't', modifiers: 14 }, rows, 'macos')).toBeNull();
    expect(commandFor({ key: 't', modifiers: 14 }, rows, 'macos', true)).toBe('devOpenTestTab');
  });
});

describe('isTypingTarget', () => {
  it('knows text fields', () => {
    document.body.innerHTML =
      '<input id="t"><input id="c" type="checkbox"><textarea id="a"></textarea><div id="e" contenteditable="true"><span id="s"></span></div><button id="b"></button>';
    const el = (id: string) => document.getElementById(id);
    expect(isTypingTarget(el('t'))).toBe(true);
    expect(isTypingTarget(el('a'))).toBe(true);
    expect(isTypingTarget(el('s'))).toBe(true);
    expect(isTypingTarget(el('c'))).toBe(false);
    expect(isTypingTarget(el('b'))).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
  });
});

describe('dispatcher', () => {
  it('runs the command of a key press and prevents the default', () => {
    const { dispatcher, ran } = setup();
    const handler = (e: Event) => dispatcher.keydown(e as KeyboardEvent);
    window.addEventListener('keydown', handler);
    const event = keydown(document.body, { key: 'n', code: 'KeyN', metaKey: true });
    window.removeEventListener('keydown', handler);
    expect(ran).toEqual(['newRDP']);
    expect(event.defaultPrevented).toBe(true);
  });

  it('ignores unbound keys and keys without modifiers', () => {
    const { dispatcher, ran } = setup();
    expect(dispatcher.keydown(new KeyboardEvent('keydown', { key: 'x', metaKey: true }))).toBe(false);
    expect(dispatcher.keydown(new KeyboardEvent('keydown', { key: 'n' }))).toBe(false);
    expect(ran).toEqual([]);
  });

  it('while typing only lets the tab, lock and Settings commands through', () => {
    const { dispatcher, ran } = setup();
    document.body.innerHTML = '<input id="field">';
    const field = document.getElementById('field')!;
    const handler = (e: Event) => dispatcher.keydown(e as KeyboardEvent);
    window.addEventListener('keydown', handler);
    const typed = keydown(field, { key: 'n', code: 'KeyN', metaKey: true });
    keydown(field, { key: 'w', code: 'KeyW', metaKey: true });
    keydown(field, { key: ',', code: 'Comma', metaKey: true });
    window.removeEventListener('keydown', handler);
    expect(typed.defaultPrevented).toBe(false);
    expect(ran).toEqual(['closeTab', 'openSettings']);
  });

  it('in a terminal only lets the session commands through; the rest goes to the terminal', () => {
    const { dispatcher, ran } = setup();
    document.body.innerHTML =
      '<div data-terminal><div class="xterm"><textarea id="helper"></textarea></div></div>';
    const helper = document.getElementById('helper')!;
    expect(isTerminalTarget(helper)).toBe(true);
    expect(isTerminalTarget(document.body)).toBe(false);
    const handler = (e: Event) => dispatcher.keydown(e as KeyboardEvent);
    window.addEventListener('keydown', handler);
    const typed = keydown(helper, { key: 'n', code: 'KeyN', metaKey: true });
    const settings = keydown(helper, { key: ',', code: 'Comma', metaKey: true });
    const clear = keydown(helper, { key: 'k', code: 'KeyK', metaKey: true });
    keydown(helper, { key: 'w', code: 'KeyW', metaKey: true });
    // Split panes are terminals too: ⌘D splits from inside one.
    keydown(helper, { key: 'd', code: 'KeyD', metaKey: true });
    window.removeEventListener('keydown', handler);
    expect(typed.defaultPrevented).toBe(false);
    expect(settings.defaultPrevented).toBe(false);
    expect(clear.defaultPrevented).toBe(true);
    expect(ran).toEqual(['clearTerminal', 'closeTab', 'splitTerminal']);
  });

  it('on an RDP canvas only lets the tab, type clipboard, reconnect and lock commands through', () => {
    const { dispatcher, ran } = setup();
    document.body.innerHTML = '<div data-rdp><canvas id="canvas" tabindex="0"></canvas></div>';
    const canvas = document.getElementById('canvas')!;
    expect(isRdpTarget(canvas)).toBe(true);
    expect(isRdpTarget(document.body)).toBe(false);
    const handler = (e: Event) => dispatcher.keydown(e as KeyboardEvent);
    window.addEventListener('keydown', handler);
    const typed = keydown(canvas, { key: 'n', code: 'KeyN', metaKey: true });
    const clear = keydown(canvas, { key: 'k', code: 'KeyK', metaKey: true });
    keydown(canvas, { key: 'w', code: 'KeyW', metaKey: true });
    window.removeEventListener('keydown', handler);
    expect(typed.defaultPrevented).toBe(false);
    expect(clear.defaultPrevented).toBe(false);
    expect(ran).toEqual(['closeTab']);
  });

  it('names the command an RDP canvas key runs', () => {
    const press = (init: KeyboardEventInit) => new KeyboardEvent('keydown', init);
    expect(rdpCommand(press({ key: 'w', code: 'KeyW', metaKey: true }), rows, 'macos')).toBe('closeTab');
    expect(rdpCommand(press({ key: 'k', code: 'KeyK', metaKey: true }), rows, 'macos')).toBeNull();
    expect(rdpCommand(press({ key: 'w', code: 'KeyW' }), rows, 'macos')).toBeNull();
  });

  it('stays quiet while the recorder listens', () => {
    const { dispatcher, ran } = setup({ recording: true });
    expect(dispatcher.keydown(new KeyboardEvent('keydown', { key: 'n', metaKey: true }))).toBe(false);
    expect(dispatcher.menuCommand('newRDP')).toBe(false);
    expect(ran).toEqual([]);
  });

  it('runs menu commands, ignoring unknown ones and the test tab outside dev builds', () => {
    const { dispatcher, ran } = setup();
    expect(dispatcher.menuCommand('lock')).toBe(true);
    expect(dispatcher.menuCommand('checkForUpdates')).toBe(true);
    expect(dispatcher.menuCommand('rm -rf')).toBe(false);
    expect(dispatcher.menuCommand('devOpenTestTab')).toBe(false);
    expect(ran).toEqual(['lock', 'checkForUpdates']);
    const dev = setup({ dev: true });
    dev.dispatcher.menuCommand('devOpenTestTab');
    expect(dev.ran).toEqual(['devOpenTestTab']);
  });

  it('drops the menu echo of a handled key press, but not a later press', () => {
    const { dispatcher, ran, advance } = setup();
    dispatcher.keydown(new KeyboardEvent('keydown', { key: 'n', metaKey: true, cancelable: true }));
    dispatcher.menuCommand('newRDP');
    advance(1000);
    dispatcher.menuCommand('newRDP');
    advance(10);
    dispatcher.menuCommand('newRDP');
    expect(ran).toEqual(['newRDP', 'newRDP', 'newRDP']);
  });

  it('runs repeated key presses', () => {
    const { dispatcher, ran } = setup();
    dispatcher.keydown(new KeyboardEvent('keydown', { key: 'w', metaKey: true, cancelable: true }));
    dispatcher.keydown(new KeyboardEvent('keydown', { key: 'w', metaKey: true, cancelable: true }));
    expect(ran).toEqual(['closeTab', 'closeTab']);
  });
});
