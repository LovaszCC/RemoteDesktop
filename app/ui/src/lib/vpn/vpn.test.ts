import { describe, expect, it, vi } from 'vitest';
import { errorMessage } from '../errors';
import { fakeBridge } from '../testing/bridge';
import type { VpnProgressDto, VpnStatusDto } from '../types';
import { createVpnStore } from './vpn.svelte';

const STATUS: VpnStatusDto = {
  supported: true,
  paired: true,
  held: [
    {
      profileId: 'P1',
      profileName: 'Office',
      externalEntries: [{ entryId: 'e2', entryName: 'web02' }],
      entryIds: ['e1', 'e2'],
      openHolders: 1,
    },
  ],
};

const progress = (phase: VpnProgressDto['phase'], requestId = 'r1'): VpnProgressDto => ({
  requestId,
  phase,
  profileName: 'Office',
  entryName: 'web01',
  unreachable: '10.20.0.15',
});

async function setup() {
  const bridge = fakeBridge({
    vpn_status: STATUS,
    vpn_cancel: null,
    vpn_release_entry: null,
    vpn_release_profile: null,
    vpn_pair: null,
    vpn_unpair: null,
    vpn_profiles: [{ id: 'P1', name: 'Office', kind: 'openvpn', state: 'disconnected' }],
  });
  const toast = { show: vi.fn() };
  const vpn = createVpnStore({ ...bridge, toast });
  await vpn.start();
  return { bridge, toast, vpn };
}

describe('vpn store', () => {
  it('progress_follows_events_and_clears_on_done', async () => {
    const { bridge, vpn } = await setup();
    bridge.emit('vpn-progress', progress('testing'));
    expect(vpn.progress).toBeNull();
    bridge.emit('vpn-progress', progress('connecting'));
    expect(vpn.progress?.phase).toBe('connecting');
    bridge.emit('vpn-progress', progress('waitingForMfa'));
    expect(vpn.progress?.phase).toBe('waitingForMfa');
    bridge.emit('vpn-progress', progress('done', 'other'));
    expect(vpn.progress?.phase).toBe('waitingForMfa');
    await vpn.cancel();
    expect(bridge.invoke).toHaveBeenCalledWith('vpn_cancel', { requestId: 'r1' });
    bridge.emit('vpn-progress', progress('failed'));
    expect(vpn.progress).toBeNull();
  });

  it('changed_refetches_status', async () => {
    const { bridge, vpn } = await setup();
    expect(vpn.status).toEqual(STATUS);
    expect(vpn.holds('e1')).toBe(true);
    expect(vpn.holds('e9')).toBe(false);
    expect(vpn.externalProfile('e2')).toBe('Office');
    expect(vpn.externalProfile('e1')).toBeNull();
    const calls = () => bridge.invoke.mock.calls.filter(([c]) => c === 'vpn_status').length;
    const before = calls();
    bridge.emit('vpn-changed', null);
    await Promise.resolve();
    expect(calls()).toBe(before + 1);
    await vpn.releaseEntry('e2');
    await vpn.releaseProfile('P1');
    expect(bridge.invoke).toHaveBeenCalledWith('vpn_release_entry', { entryId: 'e2' });
    expect(bridge.invoke).toHaveBeenCalledWith('vpn_release_profile', { profileId: 'P1' });
    expect(await vpn.profiles()).toHaveLength(1);
  });

  it('dropped_shows_a_toast', async () => {
    const { bridge, toast } = await setup();
    bridge.emit('vpn-dropped', { profileName: 'Office' });
    expect(toast.show).toHaveBeenCalledWith('The VPN "Office" went down.');
  });

  it('errors_have_texts', () => {
    for (const code of [
      'vpnNotRunning',
      'vpnNotPaired',
      'vpnNotAllowed',
      'vpnUnknownProfile',
      'vpnSetupRequired',
      'vpnCancelled',
      'vpnMfaTimeout',
      'vpnLocked',
      'vpnBusy',
      'vpnPairingDenied',
      'vpnFailed',
      'emptyVpnTestAddress',
    ]) {
      expect(errorMessage({ code, message: 'raw', detail: null })).not.toBe('raw');
    }
    expect(errorMessage({ code: 'vpnFailed', message: 'raw', detail: 'Timed out' })).toBe(
      'The VPN could not connect. (Timed out)',
    );
  });
});
