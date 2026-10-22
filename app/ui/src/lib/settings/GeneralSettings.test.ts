import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { createApi } from '../api';
import type { AppStore } from '../stores/app.svelte';
import { createSettingsStore } from '../stores/settings.svelte';
import { createTerminalsStore } from '../stores/terminals.svelte';
import { fakeBridge } from '../testing/bridge';
import { settingsDto } from '../testing/fixtures';
import type { Platform, SettingsDto } from '../types';
import { createUpdateStore } from '../updates/updates.svelte';
import { createAppearance } from '../stores/appearance.svelte';
import GeneralSettings from './GeneralSettings.svelte';

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

const settle = async () => {
  for (let i = 0; i < 3; i++) await new Promise((r) => setTimeout(r, 0));
  flushSync();
};

async function render(
  platform: Platform,
  replies: Parameters<typeof fakeBridge>[0] = {},
  appearance?: ReturnType<typeof createAppearance>,
) {
  let current: SettingsDto = settingsDto();
  const bridge = fakeBridge({
    settings_get: () => current,
    shortcuts_get: [],
    installed_terminals: [],
    accessibility_status: false,
    open_accessibility_settings: undefined,
    winbox_status: { detectedPath: '/Applications/WinBox.app', configuredExists: null, usesWine: false },
    rdp_engine_info: { name: 'FreeRDP', version: '3.32.2' },
    settings_set: (args?: Record<string, unknown>) => {
      current = { ...current, ...(args?.patch as Partial<SettingsDto>) };
      if (current.winboxPath === '') current = { ...current, winboxPath: null };
      return current;
    },
    ...replies,
  });
  const settings = createSettingsStore({ ...bridge, onLanguage: () => {} });
  await settings.start();
  const app = { platform, isUnlocked: false, databasePath: null } as unknown as AppStore;
  mounted = mount(GeneralSettings, {
    target: document.body,
    props: {
      api: createApi(bridge.invoke),
      app,
      settings,
      terminals: createTerminalsStore(bridge),
      updates: createUpdateStore(bridge),
      ...(appearance ? { appearance } : {}),
    },
  });
  await settle();
  return { bridge };
}

const button = (label: string) =>
  [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label)!;

describe('GeneralSettings embedded RDP', () => {
  it('shows the engine and its version', async () => {
    await render('linux');
    expect(document.querySelector('[data-testid="rdp-engine-version"]')?.textContent).toBe('3.32.2');
    expect(document.body.textContent).toContain('FreeRDP');
  });
});

describe('GeneralSettings sidebar', () => {
  it('chooses what a click on an entry with an open connection shows', async () => {
    const { bridge } = await render('macos');
    const select = document.querySelector<HTMLSelectElement>(
      'select[aria-label="Clicking an entry with an open connection shows"]',
    )!;
    expect(select.value).toBe('details');
    expect([...select.options].map((o) => o.textContent?.trim())).toEqual(['Details', 'Its tab']);
    select.value = 'tab';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('settings_set', { patch: { selectionShows: 'tab' } });
    expect(select.value).toBe('tab');
  });
});

describe('GeneralSettings WinBox', () => {
  it('shows the detected WinBox and discloses the argv password', async () => {
    await render('macos');
    const text = document.body.textContent ?? '';
    expect(text).toContain('/Applications/WinBox.app');
    expect(text).toContain('Automatic');
    expect(text).toContain('other users of this computer can see it in the process list');
    expect(text).not.toContain('runs through Wine');
    expect(button('Use detected').disabled).toBe(true);
  });

  it('chooses a path and goes back to the detected one', async () => {
    const { bridge } = await render('linux', {
      pick_winbox: '/home/u/Downloads/winbox64.exe',
      winbox_status: { detectedPath: null, configuredExists: true, usesWine: true },
    });
    expect(document.body.textContent).toContain('Not found');
    expect(document.body.textContent).toContain('A winbox64.exe runs through Wine.');
    button('Choose…').click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('settings_set', {
      patch: { winboxPath: '/home/u/Downloads/winbox64.exe' },
    });
    expect(document.body.textContent).toContain('/home/u/Downloads/winbox64.exe');
    button('Use detected').click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('settings_set', { patch: { winboxPath: '' } });
    expect(document.body.textContent).toContain('Automatic');
  });

  it('warns when the configured file is gone', async () => {
    await render('windows', {
      winbox_status: { detectedPath: null, configuredExists: false, usesWine: false },
    });
    expect(document.body.textContent).toContain("The file doesn't exist.");
  });
});

describe('GeneralSettings RDP autofill', () => {
  it('shows the permission, opens System Settings and checks again (macOS)', async () => {
    let trusted = false;
    const { bridge } = await render('macos', { accessibility_status: () => trusted });
    const status = () => document.querySelector('[data-testid="accessibility"]')?.textContent?.trim();
    expect(status()).toBe('Missing');
    button('Open System Settings').click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('open_accessibility_settings', undefined);
    trusted = true;
    button('Check').click();
    await settle();
    expect(status()).toBe('Granted');
  });

  it('is macOS only', async () => {
    const { bridge } = await render('windows');
    expect(document.body.textContent).not.toContain('RDP password autofill');
    expect(bridge.invoke).not.toHaveBeenCalledWith('accessibility_status', undefined);
  });
});

describe('GeneralSettings updates', () => {
  it('checks now and shows the result next to the button', async () => {
    const { bridge } = await render('windows', {
      update_check: { kind: 'upToDate', currentVersion: '0.1.0' },
    });
    expect(document.querySelector('[data-testid="update-status"]')).toBeNull();
    button('Check now').click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('update_check', { automatic: false });
    expect(document.querySelector('[data-testid="update-status"]')?.textContent).toBe(
      'RemoteDesktop is up to date.',
    );
    expect(button('Check now').disabled).toBe(false);
  });
});

describe('GeneralSettings appearance', () => {
  it('switches the theme', async () => {
    const storage = new Map<string, string>();
    const appearance = createAppearance({
      getItem: (k) => storage.get(k) ?? null,
      setItem: (k, v) => void storage.set(k, v),
    });
    await render('windows', {}, appearance);
    const select = document.querySelector<HTMLSelectElement>('select[aria-label="Theme"]')!;
    expect(select.value).toBe('system');
    expect([...select.options].map((o) => o.textContent)).toEqual(['System', 'Light', 'Dark']);
    select.value = 'dark';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    expect(appearance.theme).toBe('dark');
    expect(document.documentElement.dataset.theme).toBe('dark');
    expect(storage.get('rd.theme')).toBe('dark');
  });
});

describe('GeneralSettings system search', () => {
  it('turns the launchers on and names where they go', async () => {
    const { bridge } = await render('macos');
    const toggle = document.querySelector<HTMLButtonElement>(
      'button[role="switch"][aria-label="Show entries in system search"]',
    )!;
    expect(toggle.getAttribute('aria-checked')).toBe('false');
    expect(document.body.textContent).toContain('~/Applications/RemoteDesktop Entries');
    toggle.click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('settings_set', { patch: { showInSystemSearch: true } });
    expect(toggle.getAttribute('aria-checked')).toBe('true');
  });

  it('describes the Start menu on Windows', async () => {
    await render('windows');
    expect(document.body.textContent).toContain('RemoteDesktop Entries Start menu folder');
  });
});
