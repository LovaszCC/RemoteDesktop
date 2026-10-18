// What a key press inside an embedded terminal does (Swift `TerminalOptionKeys`, the AppDelegate key
// monitor and `BroadcastingTerminalView.onEscape`):
// - the session shortcuts (close / next / previous tab, split, broadcast, clear, copy next output, reconnect)
//   go to the app, so xterm must not swallow them;
// - Option/Alt + ← / → (nothing else held) send the readline Meta sequences `ESC b` / `ESC f`;
// - Esc cancels a running "copy next output";
// - copy / paste: Ctrl+Shift+C / V on Windows and Linux; ⌘C / ⌘V on macOS stay with the native Edit menu;
// - everything else goes to the terminal.

import { commandFor, IN_TERMINAL } from '../shortcuts/dispatcher';
import { modifierBits, shortcutFromEvent, type KeyEventLike } from '../shortcuts/keys';
import { MODIFIER, type Platform, type ShortcutCommand, type ShortcutDto } from '../types';

/** `ESC b` / `ESC f` for Option/Alt + ← / → with no other modifier, else `null`. */
export function optionKeySequence(event: KeyEventLike): string | null {
  if (modifierBits(event) !== MODIFIER.option) return null;
  if (event.key === 'ArrowLeft') return '\x1bb';
  if (event.key === 'ArrowRight') return '\x1bf';
  return null;
}

export type TerminalKeyAction =
  /** Leave it to the app's shortcut dispatcher (the event bubbles to the window). */
  | { kind: 'app'; command: ShortcutCommand }
  /** Send this text to the session instead of xterm's own encoding. */
  | { kind: 'send'; data: string }
  | { kind: 'copy' }
  | { kind: 'paste' }
  /** ⌘C / ⌘V on macOS: the native Edit menu does it. */
  | { kind: 'native' }
  | { kind: 'cancelRecording' }
  /** xterm handles it. */
  | { kind: 'terminal' };

export interface TerminalKeyContext {
  platform: Platform;
  shortcuts: readonly ShortcutDto[];
  /** Whether "copy next output" is armed or capturing. */
  recording: boolean;
}

/** Decides a key press in a terminal. */
export function terminalKeyAction(event: KeyEventLike, ctx: TerminalKeyContext): TerminalKeyAction {
  const pressed = shortcutFromEvent(event);
  if (pressed && pressed.modifiers !== 0) {
    const command = commandFor(pressed, ctx.shortcuts, ctx.platform);
    if (command && IN_TERMINAL.has(command)) {
      return { kind: 'app', command: command as ShortcutCommand };
    }
  }
  const sequence = optionKeySequence(event);
  if (sequence) return { kind: 'send', data: sequence };
  const bits = modifierBits(event);
  const key = pressed?.key;
  if (bits === 0 && key === 'escape' && ctx.recording) return { kind: 'cancelRecording' };
  if (ctx.platform === 'macos') {
    if (bits === MODIFIER.command && (key === 'c' || key === 'v')) return { kind: 'native' };
  } else if (bits === (MODIFIER.control | MODIFIER.shift)) {
    if (key === 'c') return { kind: 'copy' };
    if (key === 'v') return { kind: 'paste' };
  }
  return { kind: 'terminal' };
}
