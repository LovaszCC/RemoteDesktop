import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { createRecorderStore } from '../shortcuts/recorder.svelte';
import { createSettingsStore } from '../stores/settings.svelte';
import { fakeBridge } from '../testing/bridge';
import type { ShortcutDto } from '../types';
import ShortcutsSettings from './ShortcutsSettings.svelte';

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

const row = (over: Partial<ShortcutDto>): ShortcutDto => ({
  command: 'newRDP',
  group: 'file',
  shortcut: { key: 'n', modifiers: 1 },
  display: '⌘N',
  defaultDisplay: '⌘N',
  customized: false,
  ...over,
});

async function render(setReply: (args?: Record<string, unknown>) => unknown) {
  const rows = [
    row({}),
    row({ command: 'lock', group: 'connection', display: '⌃⌘L', defaultDisplay: '⌃⌘L' }),
  ];
  const bridge = fakeBridge({
    settings_get: { language: 'en' },
    shortcuts_get: rows,
    shortcut_set: setReply,
    shortcuts_reset_all: rows,
  });
  const settings = createSettingsStore({ ...bridge, onLanguage: () => {} });
  await settings.start();
  const recorder = createRecorderStore();
  mounted = mount(ShortcutsSettings, {
    target: document.body,
    props: { app: { platform: 'macos' }, settings, recorder },
  });
  flushSync();
  const button = (command: string) =>
    document.querySelector<HTMLButtonElement>(`[data-command="${command}"]`)!;
  const press = (init: KeyboardEventInit) =>
    window.dispatchEvent(new KeyboardEvent('keydown', { cancelable: true, ...init }));
  return { bridge, recorder, button, press };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

describe('ShortcutsSettings', () => {
  it('groups rows and keeps Restore defaults disabled without customizations', async () => {
    const { button } = await render(() => []);
    expect([...document.querySelectorAll('h3')].map((h) => h.textContent)).toEqual(['File', 'Connection']);
    expect(button('newRDP').textContent).toContain('⌘N');
    const restore = [...document.querySelectorAll('button')].find(
      (b) => b.textContent === 'Restore defaults',
    )!;
    expect(restore.disabled).toBe(true);
  });

  it('records a shortcut; Esc cancels; Backspace removes', async () => {
    const { bridge, recorder, button, press } = await render((args) => [
      row({
        shortcut: (args?.shortcut as never) ?? null,
        customized: true,
        display: args?.shortcut ? '⌥⌘J' : null,
      }),
    ]);
    button('newRDP').click();
    flushSync();
    expect(button('newRDP').textContent).toContain('Press keys…');
    press({ key: 'Escape' });
    flushSync();
    expect(recorder.recording).toBeNull();

    button('newRDP').click();
    flushSync();
    press({ key: 'j', code: 'KeyJ', metaKey: true, altKey: true });
    await settle();
    flushSync();
    expect(bridge.invoke).toHaveBeenCalledWith('shortcut_set', {
      command: 'newRDP',
      shortcut: { key: 'j', modifiers: 5 },
    });
    expect(recorder.recording).toBeNull();
    expect(document.querySelector('.reset')?.getAttribute('title')).toBe('Restore default (⌘N)');

    button('newRDP').click();
    flushSync();
    press({ key: 'Backspace' });
    await settle();
    expect(bridge.invoke).toHaveBeenLastCalledWith('shortcut_set', { command: 'newRDP', shortcut: null });
  });

  it('shows a refusal in red and keeps recording', async () => {
    const { recorder, button, press } = await render(() => {
      throw { code: 'inUse', message: 'Already used by: %@', detail: 'lock' };
    });
    button('newRDP').click();
    flushSync();
    press({ key: 'l', code: 'KeyL', metaKey: true, ctrlKey: true });
    await settle();
    flushSync();
    expect(document.querySelector('.error')?.textContent).toBe('Already used by: Lock');
    expect(recorder.recording).toBe('newRDP');
  });
});
