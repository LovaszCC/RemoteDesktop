import { describe, expect, it } from 'vitest';
import { keyName, modifierBits, sameShortcut, shortcutFromEvent } from './keys';

type Init = Partial<Pick<KeyboardEvent, 'key' | 'code' | 'metaKey' | 'shiftKey' | 'altKey' | 'ctrlKey'>>;
const press = (init: Init) => ({
  key: '',
  code: '',
  metaKey: false,
  shiftKey: false,
  altKey: false,
  ctrlKey: false,
  ...init,
});

describe('key capture', () => {
  it('maps the modifier bits: command 1, shift 2, option 4, control 8', () => {
    expect(modifierBits(press({ metaKey: true }))).toBe(1);
    expect(modifierBits(press({ shiftKey: true }))).toBe(2);
    expect(modifierBits(press({ altKey: true }))).toBe(4);
    expect(modifierBits(press({ ctrlKey: true }))).toBe(8);
    expect(modifierBits(press({ metaKey: true, shiftKey: true, altKey: true, ctrlKey: true }))).toBe(15);
  });

  it('lowercases characters', () => {
    expect(shortcutFromEvent(press({ key: 'N', code: 'KeyN', metaKey: true, shiftKey: true }))).toEqual({
      key: 'n',
      modifiers: 3,
    });
    expect(keyName(press({ key: ',', code: 'Comma' }))).toBe(',');
  });

  it('names the special keys like Rust', () => {
    const names = {
      Enter: 'return',
      Tab: 'tab',
      Backspace: 'delete',
      Escape: 'escape',
      ' ': 'space',
      ArrowLeft: 'left',
      ArrowRight: 'right',
      ArrowUp: 'up',
      ArrowDown: 'down',
      F1: 'f1',
      F12: 'f12',
    };
    for (const [key, name] of Object.entries(names)) expect(keyName(press({ key }))).toBe(name);
  });

  it('uses the physical key when Shift or Option changes the character', () => {
    expect(keyName(press({ key: '!', code: 'Digit1', shiftKey: true }))).toBe('1');
    expect(keyName(press({ key: 'Dead', code: 'KeyN', altKey: true }))).toBe('n');
    expect(keyName(press({ key: '∂', code: 'KeyD', altKey: true }))).toBe('d');
  });

  it('ignores a modifier on its own and unknown keys', () => {
    expect(shortcutFromEvent(press({ key: 'Shift', code: 'ShiftLeft', shiftKey: true }))).toBeNull();
    expect(shortcutFromEvent(press({ key: 'Meta', code: 'MetaLeft', metaKey: true }))).toBeNull();
    expect(shortcutFromEvent(press({ key: 'PageDown', code: 'PageDown' }))).toBeNull();
  });

  it('compares shortcuts', () => {
    expect(sameShortcut({ key: 'n', modifiers: 1 }, { key: 'n', modifiers: 1 })).toBe(true);
    expect(sameShortcut({ key: 'n', modifiers: 1 }, { key: 'n', modifiers: 3 })).toBe(false);
    expect(sameShortcut(null, { key: 'n', modifiers: 1 })).toBe(false);
  });
});
