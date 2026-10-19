import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { createApi } from '../api';
import { createAppStore } from '../stores/app.svelte';
import { fakeBridge } from '../testing/bridge';
import { status } from '../testing/fixtures';
import type { Platform } from '../types';
import HandoverBanner from './HandoverBanner.svelte';

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

async function render({
  handoverNotice = true,
  platform = 'macos' as Platform,
  trusted = false as boolean | null,
} = {}) {
  const bridge = fakeBridge({
    app_status: status({ handoverNotice, platform }),
    dismiss_handover: status({ handoverNotice: false, platform }),
    accessibility_status: trusted,
    open_accessibility_settings: undefined,
  });
  const app = createAppStore(bridge);
  await app.start();
  mounted = mount(HandoverBanner, { target: document.body, props: { api: createApi(bridge.invoke), app } });
  await settle();
  return bridge;
}

const banner = () => document.querySelector('.banner');
const button = (label: string) =>
  [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);

describe('HandoverBanner', () => {
  it('stays hidden without the handover flag', async () => {
    const bridge = await render({ handoverNotice: false });
    expect(banner()).toBeNull();
    expect(bridge.invoke).not.toHaveBeenCalledWith('accessibility_status', undefined);
  });

  it('asks for the Accessibility permission again while it is missing', async () => {
    const bridge = await render();
    expect(banner()?.textContent).toContain('Grant the Accessibility permission again');
    button('Open System Settings')!.click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('open_accessibility_settings', undefined);
  });

  it('only welcomes once the permission is granted, and off macOS', async () => {
    await render({ trusted: true });
    expect(banner()?.textContent).toContain('settings were carried over.');
    expect(banner()?.textContent).not.toContain('Accessibility');
    expect(button('Open System Settings')).toBeUndefined();

    unmount(mounted!);
    mounted = undefined;
    const bridge = await render({ platform: 'windows', trusted: null });
    expect(banner()?.textContent).not.toContain('Accessibility');
    expect(bridge.invoke).not.toHaveBeenCalledWith('accessibility_status', undefined);
  });

  it('Dismiss hides it', async () => {
    const bridge = await render();
    button('Dismiss')!.click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('dismiss_handover', undefined);
    expect(banner()).toBeNull();
  });
});
