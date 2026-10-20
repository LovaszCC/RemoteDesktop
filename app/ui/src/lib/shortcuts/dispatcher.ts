// The global shortcut dispatcher: a window `keydown` listener and the native menu's `menu-command`
// events both end up here and run the same app command.
//
// Rules:
// - nothing fires while the shortcut recorder listens;
// - while typing in a text field only the tab and lock commands fire (and Settings);
// - while an embedded terminal (also a split pane) has the focus only the session commands fire (close /
//   next / previous tab, split, broadcast to all panes, clear, copy next output, reconnect, new local
//   terminal); every other key goes to the terminal (Swift key monitor);
// - while an RDP canvas has the focus only the tab, type clipboard, reconnect and lock commands fire; every
//   other key goes to the remote session;
// - on macOS the menu's key equivalent and the webview's keydown can both report one key press, so the same
//   command from the other source within a moment is dropped.

import type { KeyShortcut, Platform, ShortcutCommand, ShortcutDto } from '../types';
import { MODIFIER } from '../types';
import { sameShortcut, shortcutFromEvent } from './keys';

/**
 * Shortcut commands plus the app's own: Settings, Check for Updates, Export, the export and CSV imports and, in dev
 * builds, the test tab.
 */
export type AppCommand =
  | ShortcutCommand
  | 'openSettings'
  | 'checkForUpdates'
  | 'devOpenTestTab'
  | 'exportLibrary'
  | 'importTransfer'
  | 'importCsv';

export const SHORTCUT_COMMANDS: readonly ShortcutCommand[] = [
  'newRDP',
  'newSSH',
  'newWeb',
  'newMikrotik',
  'newFolder',
  'newCredential',
  'importRDM',
  'importWinbox',
  'connect',
  'newLocalTerminal',
  'typeClipboard',
  'splitTerminal',
  'broadcastAll',
  'clearTerminal',
  'recordOutput',
  'openFiles',
  'closeTab',
  'nextTab',
  'previousTab',
  'reconnect',
  'lock',
  'toggleSidebar',
];

const APP_COMMANDS = new Set<string>([
  ...SHORTCUT_COMMANDS,
  'openSettings',
  'checkForUpdates',
  'devOpenTestTab',
  'exportLibrary',
  'importTransfer',
  'importCsv',
]);

export function isAppCommand(value: unknown): value is AppCommand {
  return typeof value === 'string' && APP_COMMANDS.has(value);
}

/** Commands that still fire while the focus is in a text field. */
export const WHILE_TYPING: ReadonlySet<AppCommand> = new Set<AppCommand>([
  'closeTab',
  'nextTab',
  'previousTab',
  'lock',
  'openSettings',
  'toggleSidebar',
]);

/** Commands that fire while an embedded terminal has the focus. */
export const IN_TERMINAL: ReadonlySet<AppCommand> = new Set<AppCommand>([
  'closeTab',
  'nextTab',
  'previousTab',
  'splitTerminal',
  'broadcastAll',
  'clearTerminal',
  'recordOutput',
  'reconnect',
  'newLocalTerminal',
  'toggleSidebar',
]);

/** Commands that fire while an RDP canvas has the focus (the view lets these keys through). */
export const IN_RDP: ReadonlySet<AppCommand> = new Set<AppCommand>([
  'closeTab',
  'nextTab',
  'previousTab',
  'typeClipboard',
  'reconnect',
  'lock',
  'toggleSidebar',
]);

/** Whether the focus is inside an RDP view (its element carries `data-rdp`). */
export function isRdpTarget(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest('[data-rdp]') !== null;
}

/** Whether the focus is inside an embedded terminal (its element carries `data-terminal`). */
export function isTerminalTarget(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest('[data-terminal]') !== null;
}

/** Settings: ⌘, on macOS, Ctrl+, elsewhere (not customisable). */
export function settingsShortcut(platform: Platform): KeyShortcut {
  return { key: ',', modifiers: platform === 'macos' ? MODIFIER.command : MODIFIER.control };
}

/** Dev builds: Ctrl+Alt+Shift+T opens a test tab. */
export const TEST_TAB_SHORTCUT: KeyShortcut = {
  key: 't',
  modifiers: MODIFIER.control | MODIFIER.option | MODIFIER.shift,
};

/** The command bound to `pressed`, if any. */
export function commandFor(
  pressed: KeyShortcut,
  rows: readonly ShortcutDto[],
  platform: Platform,
  dev = false,
): AppCommand | null {
  const row = rows.find((r) => sameShortcut(r.shortcut, pressed));
  if (row) return row.command;
  if (sameShortcut(settingsShortcut(platform), pressed)) return 'openSettings';
  if (dev && sameShortcut(TEST_TAB_SHORTCUT, pressed)) return 'devOpenTestTab';
  return null;
}

/** The app command a key press on an RDP canvas runs instead of reaching the session, if any. */
export function rdpCommand(
  event: KeyboardEvent,
  rows: readonly ShortcutDto[],
  platform: Platform,
): AppCommand | null {
  const pressed = shortcutFromEvent(event);
  if (!pressed || pressed.modifiers === 0) return null;
  const command = commandFor(pressed, rows, platform);
  return command && IN_RDP.has(command) ? command : null;
}

/** Whether the focus is somewhere the user types: an input, a text area, a select or editable content. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  if (target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) return true;
  if (target instanceof HTMLInputElement) {
    return !['button', 'checkbox', 'radio', 'submit', 'reset', 'range', 'color', 'file'].includes(
      target.type,
    );
  }
  return target instanceof HTMLElement && (target.isContentEditable || !!target.closest('[contenteditable]'));
}

export interface DispatcherOptions {
  /** The effective shortcuts (the settings store's rows). */
  shortcuts: () => readonly ShortcutDto[];
  platform: () => Platform;
  /** Whether the shortcut recorder is listening. */
  isRecording: () => boolean;
  run: (command: AppCommand) => void;
  /** Enables the dev-only test tab. */
  dev?: boolean;
  now?: () => number;
  /** How long a command from the other source counts as the same key press, in ms. */
  dedupeWindow?: number;
}

type Source = 'key' | 'menu';

export function createDispatcher(options: DispatcherOptions) {
  const now = options.now ?? (() => Date.now());
  const dedupeMs = options.dedupeWindow ?? 250;
  let last: { command: AppCommand; source: Source; at: number } | null = null;

  function fire(command: AppCommand, source: Source): boolean {
    const at = now();
    if (last && last.command === command && last.source !== source && at - last.at < dedupeMs) {
      last = null;
      return true;
    }
    last = { command, source, at };
    options.run(command);
    return true;
  }

  return {
    /** Handles a key press; returns whether it ran a command (the event is then prevented). */
    keydown(event: KeyboardEvent): boolean {
      if (event.defaultPrevented || event.isComposing || options.isRecording()) return false;
      const pressed = shortcutFromEvent(event);
      if (!pressed || pressed.modifiers === 0) return false;
      const command = commandFor(pressed, options.shortcuts(), options.platform(), options.dev);
      if (!command) return false;
      if (isTerminalTarget(event.target)) {
        if (!IN_TERMINAL.has(command)) return false;
      } else if (isRdpTarget(event.target)) {
        if (!IN_RDP.has(command)) return false;
      } else if (isTypingTarget(event.target) && !WHILE_TYPING.has(command)) return false;
      event.preventDefault();
      return fire(command, 'key');
    },
    /** A native menu click (or its key equivalent). Unknown commands are ignored. */
    menuCommand(command: unknown): boolean {
      if (options.isRecording() || !isAppCommand(command)) return false;
      if (command === 'devOpenTestTab' && !options.dev) return false;
      return fire(command, 'menu');
    },
  };
}

export type Dispatcher = ReturnType<typeof createDispatcher>;
