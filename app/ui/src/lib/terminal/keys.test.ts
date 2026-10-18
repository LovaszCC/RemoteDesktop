import { describe, expect, it } from 'vitest';
import type { KeyEventLike } from '../shortcuts/keys';
import type { Platform, ShortcutDto } from '../types';
import { optionKeySequence, terminalKeyAction, type TerminalKeyContext } from './keys';

function key(init: Partial<KeyEventLike> & { key: string }): KeyEventLike {
  return { code: '', metaKey: false, shiftKey: false, altKey: false, ctrlKey: false, ...init };
}

const row = (command: ShortcutDto['command'], k: string, modifiers: number): ShortcutDto => ({
  command,
  group: 'connection',
  shortcut: { key: k, modifiers },
  display: null,
  defaultDisplay: '',
  customized: false,
});

const shortcuts: ShortcutDto[] = [
  row('closeTab', 'w', 1),
  row('nextTab', 'tab', 8),
  row('previousTab', 'tab', 10),
  row('clearTerminal', 'k', 1),
  row('recordOutput', 'r', 3),
  row('reconnect', 'r', 1),
  row('newSSH', 'n', 3),
  row('lock', 'l', 9),
];

function ctx(platform: Platform = 'macos', recording = false): TerminalKeyContext {
  return { platform, shortcuts, recording };
}

describe('optionKeySequence (Swift TerminalOptionKeys)', () => {
  it('sends ESC b / ESC f for Option+← / → alone', () => {
    expect(optionKeySequence(key({ key: 'ArrowLeft', altKey: true }))).toBe('\x1bb');
    expect(optionKeySequence(key({ key: 'ArrowRight', altKey: true }))).toBe('\x1bf');
  });

  it('leaves everything else to the terminal', () => {
    expect(optionKeySequence(key({ key: 'ArrowLeft' }))).toBeNull();
    expect(optionKeySequence(key({ key: 'ArrowLeft', altKey: true, shiftKey: true }))).toBeNull();
    expect(optionKeySequence(key({ key: 'ArrowRight', altKey: true, metaKey: true }))).toBeNull();
    expect(optionKeySequence(key({ key: 'ArrowUp', altKey: true }))).toBeNull();
    expect(optionKeySequence(key({ key: '@', code: 'KeyG', altKey: true }))).toBeNull();
  });
});

describe('terminalKeyAction', () => {
  it('hands the session shortcuts to the app', () => {
    expect(terminalKeyAction(key({ key: 'w', code: 'KeyW', metaKey: true }), ctx())).toEqual({
      kind: 'app',
      command: 'closeTab',
    });
    expect(terminalKeyAction(key({ key: 'Tab', ctrlKey: true }), ctx()).kind).toBe('app');
    expect(terminalKeyAction(key({ key: 'Tab', ctrlKey: true, shiftKey: true }), ctx())).toEqual({
      kind: 'app',
      command: 'previousTab',
    });
    for (const [k, mods] of [
      ['k', { metaKey: true }],
      ['R', { metaKey: true, shiftKey: true }],
      ['r', { metaKey: true }],
    ] as const) {
      expect(terminalKeyAction(key({ key: k, code: `Key${k.toUpperCase()}`, ...mods }), ctx()).kind).toBe(
        'app',
      );
    }
  });

  it('keeps other app shortcuts and plain keys in the terminal', () => {
    expect(
      terminalKeyAction(key({ key: 'N', code: 'KeyN', metaKey: true, shiftKey: true }), ctx()).kind,
    ).toBe('terminal');
    expect(terminalKeyAction(key({ key: 'l', code: 'KeyL', metaKey: true, ctrlKey: true }), ctx()).kind).toBe(
      'terminal',
    );
    expect(terminalKeyAction(key({ key: 'c', code: 'KeyC', ctrlKey: true }), ctx('linux')).kind).toBe(
      'terminal',
    );
    expect(terminalKeyAction(key({ key: 'a' }), ctx()).kind).toBe('terminal');
  });

  it('sends the Option word jumps', () => {
    expect(terminalKeyAction(key({ key: 'ArrowLeft', altKey: true }), ctx())).toEqual({
      kind: 'send',
      data: '\x1bb',
    });
    expect(terminalKeyAction(key({ key: 'ArrowRight', altKey: true }), ctx('windows'))).toEqual({
      kind: 'send',
      data: '\x1bf',
    });
  });

  it('cancels a recording on Esc only while recording', () => {
    expect(terminalKeyAction(key({ key: 'Escape' }), ctx('macos', true)).kind).toBe('cancelRecording');
    expect(terminalKeyAction(key({ key: 'Escape' }), ctx('macos', false)).kind).toBe('terminal');
    expect(terminalKeyAction(key({ key: 'Escape', shiftKey: true }), ctx('macos', true)).kind).toBe(
      'terminal',
    );
  });

  it('copies and pastes with Ctrl+Shift+C/V on Windows and Linux, ⌘C/⌘V natively on macOS', () => {
    const copy = key({ key: 'C', code: 'KeyC', ctrlKey: true, shiftKey: true });
    const paste = key({ key: 'V', code: 'KeyV', ctrlKey: true, shiftKey: true });
    expect(terminalKeyAction(copy, ctx('windows')).kind).toBe('copy');
    expect(terminalKeyAction(paste, ctx('linux')).kind).toBe('paste');
    expect(terminalKeyAction(copy, ctx('macos')).kind).toBe('terminal');
    expect(terminalKeyAction(key({ key: 'c', code: 'KeyC', metaKey: true }), ctx('macos')).kind).toBe(
      'native',
    );
    expect(terminalKeyAction(key({ key: 'v', code: 'KeyV', metaKey: true }), ctx('macos')).kind).toBe(
      'native',
    );
  });
});
