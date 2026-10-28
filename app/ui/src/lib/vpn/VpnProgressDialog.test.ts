import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fakeBridge } from '../testing/bridge';
import type { VpnProgressDto } from '../types';
import { createVpnStore } from './vpn.svelte';
import VpnProgressDialog from './VpnProgressDialog.svelte';

let mounted: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

const progress = (phase: VpnProgressDto['phase']): VpnProgressDto => ({
  requestId: 'r1',
  phase,
  profileName: 'Office',
  entryName: 'web01',
  unreachable: '10.20.0.15',
});

describe('VpnProgressDialog', () => {
  it('dialog_shows_phases_and_cancels', async () => {
    const bridge = fakeBridge({ vpn_status: { supported: true, paired: true, held: [] }, vpn_cancel: null });
    const vpn = createVpnStore({ ...bridge, toast: { show: vi.fn() } });
    await vpn.start();
    mounted = mount(VpnProgressDialog, { target: document.body, props: { vpn } });
    flushSync();
    const dialog = () => document.querySelector('[role="dialog"]');
    expect(dialog()).toBeNull();

    bridge.emit('vpn-progress', progress('connecting'));
    flushSync();
    expect(dialog()?.textContent).toContain('Connecting VPN "Office"…');
    expect(dialog()?.textContent).toContain('10.20.0.15 did not answer the ping.');

    bridge.emit('vpn-progress', progress('waitingForMfa'));
    flushSync();
    expect(dialog()?.textContent).toContain('MacVPN needs your sign-in code — look at its window.');
    bridge.emit('vpn-progress', progress('waitingForCredentials'));
    flushSync();
    expect(dialog()?.textContent).toContain('MacVPN needs your password — look at its window.');

    [...document.querySelectorAll<HTMLButtonElement>('button')]
      .find((b) => b.textContent?.trim() === 'Cancel')!
      .click();
    await Promise.resolve();
    expect(bridge.invoke).toHaveBeenCalledWith('vpn_cancel', { requestId: 'r1' });

    bridge.emit('vpn-progress', progress('failed'));
    flushSync();
    expect(dialog()).toBeNull();
  });
});
