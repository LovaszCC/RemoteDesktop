import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fakeBridge } from '../testing/bridge';
import type { VpnStatusDto } from '../types';
import { createVpnStore } from '../vpn/vpn.svelte';
import { settingsTabs } from './model';
import VpnSettings from './VpnSettings.svelte';

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

const HELD: VpnStatusDto = {
  supported: true,
  paired: true,
  held: [
    {
      profileId: 'P1',
      profileName: 'Office',
      externalEntries: [{ entryId: 'e2', entryName: 'web02' }],
      entryIds: ['e1', 'e2'],
      openHolders: 2,
    },
  ],
};

async function render(status: VpnStatusDto, replies: Parameters<typeof fakeBridge>[0] = {}) {
  let current = status;
  const bridge = fakeBridge({
    vpn_status: () => current,
    vpn_pair: () => {
      current = { ...current, paired: true };
    },
    vpn_unpair: () => {
      current = { ...current, paired: false };
    },
    vpn_release_profile: null,
    ...replies,
  });
  const vpn = createVpnStore({ ...bridge, toast: { show: vi.fn() } });
  await vpn.start();
  const confirm = vi.fn(async () => true);
  const alerts = { show: vi.fn() };
  mounted = mount(VpnSettings, { target: document.body, props: { vpn, dialogs: { confirm }, alerts } });
  await settle();
  return { bridge, confirm, alerts };
}

const text = () => document.body.textContent ?? '';
const button = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label);

describe('VpnSettings', () => {
  it('shows_pairing_state', async () => {
    await render({ supported: true, paired: false, held: [] });
    expect(text()).toContain('Not paired.');
    expect(text()).toContain('Allow other apps to control VPNs');
    expect(button('Pair with MacVPN…')).toBeTruthy();
    expect(text()).toContain('No VPN is held.');
  });

  it('pair_calls_the_command_and_refreshes', async () => {
    const { bridge } = await render({ supported: true, paired: false, held: [] });
    button('Pair with MacVPN…')!.click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('vpn_pair', undefined);
    expect(text()).toContain('Paired with MacVPN');
    expect(button('Pair again…')).toBeTruthy();
  });

  it('unpair_asks_first', async () => {
    const { bridge, confirm } = await render(HELD);
    button('Unpair')!.click();
    await settle();
    expect(confirm).toHaveBeenCalled();
    expect(bridge.invoke).toHaveBeenCalledWith('vpn_unpair', undefined);
    expect(text()).toContain('Not paired.');
  });

  it('a_failed_pairing_shows_the_error', async () => {
    const { alerts } = await render(
      { supported: true, paired: false, held: [] },
      {
        vpn_pair: () => {
          throw { code: 'vpnPairingDenied', message: 'MacVPN did not allow the pairing.', detail: null };
        },
      },
    );
    button('Pair with MacVPN…')!.click();
    await settle();
    expect(alerts.show).toHaveBeenCalledWith('MacVPN did not allow the pairing.');
  });

  it('release_releases_the_profile', async () => {
    const { bridge } = await render(HELD);
    expect(text()).toContain('Office');
    expect(text()).toContain('web02');
    button('Release')!.click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('vpn_release_profile', { profileId: 'P1' });
  });

  it('open_holders_are_explained', async () => {
    await render(HELD);
    expect(text()).toContain('Still used by 2 open tabs');
  });

  it('tab_hidden_off_macos', () => {
    expect(settingsTabs('macos').map((tab) => tab.value)).toEqual(['general', 'shortcuts', 'vpn']);
    expect(settingsTabs('windows').map((tab) => tab.value)).toEqual(['general', 'shortcuts']);
  });
});
