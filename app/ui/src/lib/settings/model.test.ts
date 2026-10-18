import { describe, expect, it } from 'vitest';
import type { ShortcutDto } from '../types';
import {
  canChangePassword,
  lastCheckedText,
  parseTimestamp,
  recorderAction,
  recorderCaption,
  rememberKeyLabel,
  revealLabel,
  shortcutGroups,
} from './model';

const key = (init: Partial<KeyboardEvent>) =>
  ({
    key: '',
    code: '',
    metaKey: false,
    shiftKey: false,
    altKey: false,
    ctrlKey: false,
    ...init,
  }) as KeyboardEvent;

describe('settings model', () => {
  it('enables Change only when every field is filled and the new ones match', () => {
    expect(canChangePassword({ current: 'a', next: 'b', again: 'b' })).toBe(true);
    expect(canChangePassword({ current: '', next: 'b', again: 'b' })).toBe(false);
    expect(canChangePassword({ current: 'a', next: '', again: '' })).toBe(false);
    expect(canChangePassword({ current: 'a', next: 'b', again: 'c' })).toBe(false);
  });

  it('names the file manager and the keyring per platform', () => {
    expect(revealLabel('macos')).toBe('Show in Finder');
    expect(revealLabel('windows')).toBe('Show in Explorer');
    expect(revealLabel('linux')).toBe('Show in File Manager');
    expect(rememberKeyLabel('macos')).toContain('Keychain');
    expect(rememberKeyLabel('linux')).toBe('Remember the master key on this computer');
    expect(recorderCaption('macos')).toContain('⌫');
    expect(recorderCaption('windows')).toContain('Backspace');
  });

  it('reads the stored timestamp', () => {
    expect(parseTimestamp('2026-01-02 03:04:05.000')).toBe(Date.UTC(2026, 0, 2, 3, 4, 5));
    expect(parseTimestamp('nonsense')).toBeNull();
    expect(parseTimestamp(null)).toBeNull();
    const now = Date.UTC(2026, 0, 2, 3, 9, 5);
    expect(lastCheckedText('2026-01-02 03:04:05.000', 'en', now)).toBe('Last checked: 5 minutes ago');
    expect(lastCheckedText(null, 'en', now)).toBeNull();
  });

  it('groups the shortcuts like the menus', () => {
    const rows = [
      { command: 'connect', group: 'connection' },
      { command: 'newRDP', group: 'file' },
    ] as ShortcutDto[];
    const groups = shortcutGroups(rows);
    expect(groups.map((g) => g.title)).toEqual(['File', 'Connection']);
    expect(groups[0]!.rows[0]!.command).toBe('newRDP');
  });

  it('maps recorder key presses', () => {
    expect(recorderAction(key({ key: 'Escape' }))).toEqual({ kind: 'cancel' });
    expect(recorderAction(key({ key: 'Backspace' }))).toEqual({ kind: 'remove' });
    expect(recorderAction(key({ key: 'Delete', code: 'Delete' }))).toEqual({ kind: 'remove' });
    expect(recorderAction(key({ key: 'Shift', code: 'ShiftLeft', shiftKey: true }))).toEqual({
      kind: 'ignore',
    });
    expect(recorderAction(key({ key: 'Backspace', metaKey: true }))).toEqual({
      kind: 'set',
      shortcut: { key: 'delete', modifiers: 1 },
    });
    expect(recorderAction(key({ key: 'k', code: 'KeyK', ctrlKey: true }))).toEqual({
      kind: 'set',
      shortcut: { key: 'k', modifiers: 8 },
    });
    expect(recorderAction(key({ key: 'k', code: 'KeyK' }))).toEqual({
      kind: 'set',
      shortcut: { key: 'k', modifiers: 0 },
    });
  });
});
