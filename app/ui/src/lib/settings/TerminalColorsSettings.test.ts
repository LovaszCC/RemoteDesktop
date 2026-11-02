import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { createAppearance } from '../stores/appearance.svelte';
import { TERMINAL_DEFAULTS, type TerminalMode } from '../terminal/colors';
import TerminalColorsSettings from './TerminalColorsSettings.svelte';

let mounted: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
  document.getElementById('terminal-colors')?.remove();
});

function render(initialMode: TerminalMode = 'dark') {
  const storage = new Map<string, string>();
  const appearance = createAppearance({
    getItem: (k) => storage.get(k) ?? null,
    setItem: (k, v) => void storage.set(k, v),
  });
  mounted = mount(TerminalColorsSettings, { target: document.body, props: { appearance, initialMode } });
  flushSync();
  const picker = (key: string) => document.querySelector<HTMLInputElement>(`input[data-key="${key}"]`)!;
  const button = (text: string) =>
    [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === text)!;
  const change = (key: string, value: string) => {
    picker(key).value = value;
    picker(key).dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
  };
  return { appearance, picker, button, change };
}

describe('TerminalColorsSettings', () => {
  it('opens on the given mode with its Catppuccin colours, grouped', () => {
    const { picker, button } = render('dark');
    expect(button('Dark').getAttribute('aria-checked')).toBe('true');
    expect(picker('background').value).toBe(TERMINAL_DEFAULTS.dark.background);
    expect(picker('brightWhite').value).toBe(TERMINAL_DEFAULTS.dark.brightWhite);
    expect(document.querySelectorAll('input[type="color"]')).toHaveLength(20);
    expect([...document.querySelectorAll('.group-title')].map((e) => e.textContent)).toEqual([
      'Basic',
      'Normal',
      'Bright',
    ]);
    button('Light').click();
    flushSync();
    expect(picker('background').value).toBe(TERMINAL_DEFAULTS.light.background);
  });

  it('changes a colour of the shown mode and keeps the selection alpha', () => {
    const { appearance, change } = render('light');
    change('red', '#ff0000');
    change('selectionBackground', '#112233');
    expect(appearance.terminalColors.light).toEqual({ red: '#ff0000', selectionBackground: '#11223340' });
    expect(appearance.terminalColors.dark).toEqual({});
    expect(document.querySelector<HTMLElement>('.preview')!.style.background).not.toBe('');
  });

  it('resets the shown mode, enabled only with changes', () => {
    const { appearance, button, change, picker } = render('dark');
    const reset = () => button('Reset to Catppuccin Mocha');
    expect(reset().disabled).toBe(true);
    change('cyan', '#00ffff');
    expect(reset().disabled).toBe(false);
    reset().click();
    flushSync();
    expect(appearance.terminalColors.dark).toEqual({});
    expect(picker('cyan').value).toBe(TERMINAL_DEFAULTS.dark.cyan);
    button('Light').click();
    flushSync();
    expect(button('Reset to Catppuccin Latte').disabled).toBe(true);
  });
});
