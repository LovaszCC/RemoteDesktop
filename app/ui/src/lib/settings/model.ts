// Pure helpers of the Settings sheet (Swift `SettingsView`, `ShortcutsSettingsView`).

import { t } from '../i18n/index.svelte';
import { relativeAge } from '../format';
import { modifierBits, shortcutFromEvent, type KeyEventLike } from '../shortcuts/keys';
import type { KeyShortcut, Platform, SelectionShows, ShortcutDto, Timestamp } from '../types';

/** The language picker: native names, not translated (Swift `AppLanguage.displayName`). */
export const LANGUAGE_OPTIONS = [
  { value: 'en', label: 'English' },
  { value: 'hu', label: 'Magyar' },
] as const;

/** "Clicking an entry with an open connection shows": Details (the default) or the connection's tab. */
export function selectionShowsOptions(): { value: SelectionShows; label: string }[] {
  return [
    { value: 'details', label: t('Details') },
    { value: 'tab', label: t('Its tab') },
  ];
}

export interface PasswordChange {
  current: string;
  next: string;
  again: string;
}

/** "Change" is enabled only when all three fields are filled and the new ones match. */
export function canChangePassword({ current, next, again }: PasswordChange): boolean {
  return current !== '' && next !== '' && next === again;
}

/** "Show in Finder" / "Show in Explorer" / "Show in File Manager". */
export function revealLabel(platform: Platform): string {
  switch (platform) {
    case 'macos':
      return t('Show in Finder');
    case 'windows':
      return t('Show in Explorer');
    case 'linux':
      return t('Show in File Manager');
  }
}

/** The "Remember the master key" toggle, naming the OS keyring. */
export function rememberKeyLabel(platform: Platform): string {
  return platform === 'macos'
    ? t('Remember the master key on this Mac (Keychain)')
    : t('Remember the master key on this computer');
}

/** `yyyy-MM-dd HH:mm:ss.SSS` (UTC) → epoch ms, or `null` when unreadable. */
export function parseTimestamp(value: Timestamp | null): number | null {
  if (!value) return null;
  const ms = Date.parse(`${value.replace(' ', 'T')}Z`);
  return Number.isNaN(ms) ? null : ms;
}

/** "Last checked: 5 minutes ago", or `null` when never checked. */
export function lastCheckedText(value: Timestamp | null, locale: string, now = Date.now()): string | null {
  const at = parseTimestamp(value);
  if (at === null) return null;
  return t('Last checked: %@', relativeAge((now - at) / 1000, locale));
}

/** Shortcut rows grouped like the menus: File, then Connection. */
export function shortcutGroups(rows: readonly ShortcutDto[]) {
  return [
    { title: t('File'), rows: rows.filter((r) => r.group === 'file') },
    { title: t('Connection'), rows: rows.filter((r) => r.group === 'connection') },
  ];
}

/** The footer caption, with the platform's key names. */
export function recorderCaption(platform: Platform): string {
  return platform === 'macos'
    ? t('Click a shortcut, then press the new keys. ⎋ cancels, ⌫ removes the shortcut.')
    : t('Click a shortcut, then press the new keys. Esc cancels, Backspace removes the shortcut.');
}

export type RecorderAction =
  { kind: 'cancel' } | { kind: 'remove' } | { kind: 'set'; shortcut: KeyShortcut } | { kind: 'ignore' };

/** What a key press does while recording: Esc cancels, ⌫/Delete alone removes, anything else is tried. */
export function recorderAction(event: KeyEventLike): RecorderAction {
  const bare = modifierBits(event) === 0;
  if (bare && event.key === 'Delete') return { kind: 'remove' };
  const pressed = shortcutFromEvent(event);
  if (!pressed) return { kind: 'ignore' };
  if (bare && pressed.key === 'escape') return { kind: 'cancel' };
  if (bare && pressed.key === 'delete') return { kind: 'remove' };
  return { kind: 'set', shortcut: pressed };
}

/** The WinBox path row: the configured path, else "Automatic". */
export function winboxPathText(configured: string | null): string {
  return configured ?? t('Automatic');
}

/** The detected WinBox row: the path, else "Not found". */
export function winboxDetectedText(detected: string | null | undefined): string {
  return detected ?? t('Not found');
}
