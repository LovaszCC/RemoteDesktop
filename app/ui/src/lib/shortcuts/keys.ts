// Key presses as `KeyShortcut`s in the Rust format (`rd-core::shortcuts`): a lowercased character or a
// special key name, and the modifier bits command 1 (⌘ on macOS, Super elsewhere), shift 2, option/alt 4,
// control 8.

import { MODIFIER, type KeyShortcut } from '../types';

/** `KeyboardEvent.key` names of the special keys, mapped to the Rust names. */
const SPECIAL: Record<string, string> = {
  Enter: 'return',
  Tab: 'tab',
  Backspace: 'delete',
  Escape: 'escape',
  ' ': 'space',
  Spacebar: 'space',
  ArrowLeft: 'left',
  ArrowRight: 'right',
  ArrowUp: 'up',
  ArrowDown: 'down',
};

const MODIFIER_KEYS = new Set(['Shift', 'Control', 'Alt', 'Meta', 'OS', 'AltGraph', 'CapsLock', 'Fn']);

export type KeyEventLike = Pick<
  KeyboardEvent,
  'key' | 'code' | 'metaKey' | 'shiftKey' | 'altKey' | 'ctrlKey'
>;

/** The modifier bits held during `event`. */
export function modifierBits(event: KeyEventLike): number {
  return (
    (event.metaKey ? MODIFIER.command : 0) |
    (event.shiftKey ? MODIFIER.shift : 0) |
    (event.altKey ? MODIFIER.option : 0) |
    (event.ctrlKey ? MODIFIER.control : 0)
  );
}

/** The Rust key name of `event`, or `null` for a modifier on its own or a key the format can't hold. */
export function keyName(event: KeyEventLike): string | null {
  const { key, code } = event;
  if (MODIFIER_KEYS.has(key) || key === 'Dead' || key === 'Unidentified') {
    return codeKey(code);
  }
  if (key in SPECIAL) return SPECIAL[key] ?? null;
  if (/^F([1-9]|1[0-2])$/.test(key)) return key.toLowerCase();
  // Plain letters and digits as typed; with Shift or Option the produced character differs ("!", "˜"),
  // so the physical key decides.
  if (/^[a-zA-Z0-9]$/.test(key)) return key.toLowerCase();
  const physical = codeKey(code);
  if (physical) return physical;
  if ([...key].length === 1) return key.toLowerCase();
  return null;
}

/** `KeyN` → `n`, `Digit1` → `1`, `Comma` → `,`… (the US position of a physical key). */
function codeKey(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1]!.toLowerCase();
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return digit[1]!;
  return CODE_PUNCTUATION[code] ?? null;
}

const CODE_PUNCTUATION: Record<string, string> = {
  Comma: ',',
  Period: '.',
  Slash: '/',
  Semicolon: ';',
  Quote: "'",
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  Minus: '-',
  Equal: '=',
  Backquote: '`',
};

/** The shortcut of a key press, or `null` for a bare modifier. */
export function shortcutFromEvent(event: KeyEventLike): KeyShortcut | null {
  const key = keyName(event);
  return key === null ? null : { key, modifiers: modifierBits(event) };
}

export function sameShortcut(a: KeyShortcut | null | undefined, b: KeyShortcut | null | undefined): boolean {
  return !!a && !!b && a.key === b.key && a.modifiers === b.modifiers;
}
