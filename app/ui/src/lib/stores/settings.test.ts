import { describe, expect, it, vi } from 'vitest';
import { fakeBridge } from '../testing/bridge';
import { settingsDto, shortcutRow } from '../testing/fixtures';
import { createSettingsStore } from './settings.svelte';

function setup() {
  const bridge = fakeBridge({
    settings_get: settingsDto(),
    shortcuts_get: [shortcutRow('newRDP'), shortcutRow('lock', true)],
    settings_set: (args?: Record<string, unknown>) => settingsDto(args?.patch as object),
    shortcut_set: [shortcutRow('newRDP', true)],
    shortcut_reset: [shortcutRow('newRDP')],
    shortcuts_reset_all: [shortcutRow('newRDP')],
  });
  const onLanguage = vi.fn();
  return { bridge, onLanguage, store: createSettingsStore({ ...bridge, onLanguage }) };
}

describe('settings store', () => {
  it('loads settings and shortcuts and applies the language', async () => {
    const { store, onLanguage } = setup();
    await store.start();
    expect(store.settings?.language).toBe('en');
    expect(onLanguage).toHaveBeenCalledWith('en');
    expect(store.customizedCount).toBe(1);
    expect(store.shortcut('lock')?.customized).toBe(true);
  });

  it('follows the change events', async () => {
    const { store, bridge, onLanguage } = setup();
    await store.start();
    bridge.emit('settings-changed', settingsDto({ language: 'hu' }));
    expect(onLanguage).toHaveBeenLastCalledWith('hu');
    bridge.emit('shortcuts-changed', []);
    expect(store.shortcuts).toEqual([]);
    store.stop();
    expect(bridge.listenerCount('settings-changed')).toBe(0);
    expect(bridge.listenerCount('shortcuts-changed')).toBe(0);
  });

  it('switches the language before Rust answers', async () => {
    const { store, bridge, onLanguage } = setup();
    const pending = store.setLanguage('hu');
    expect(onLanguage).toHaveBeenCalledWith('hu');
    await pending;
    expect(bridge.invoke).toHaveBeenLastCalledWith('settings_set', { patch: { language: 'hu' } });
    expect(store.settings?.language).toBe('hu');
  });

  it('sets and resets shortcuts', async () => {
    const { store, bridge } = setup();
    await store.setShortcut('newRDP', { key: 'r', modifiers: 9 });
    expect(bridge.invoke).toHaveBeenLastCalledWith('shortcut_set', {
      command: 'newRDP',
      shortcut: { key: 'r', modifiers: 9 },
    });
    expect(store.customizedCount).toBe(1);
    await store.resetShortcut('newRDP');
    expect(store.customizedCount).toBe(0);
    await store.resetAllShortcuts();
    expect(bridge.invoke).toHaveBeenLastCalledWith('shortcuts_reset_all', undefined);
  });
});
