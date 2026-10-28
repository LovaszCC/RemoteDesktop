// Connect with VPN (macOS): the held VPNs and pairing from `vpn_status`, kept fresh by `vpn-changed`; the
// progress of the connect whose VPN is coming up from `vpn-progress` (the dialog shows it; `testing` is
// too short to show); a toast when a held VPN drops (`vpn-dropped`). Rust holds and releases the VPNs.

import { onVpnChanged, onVpnDropped, onVpnProgress, type Unlisten } from '../api';
import { t } from '../i18n/index.svelte';
import { resolveDeps, type StoreDeps } from '../stores/deps';
import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
import type { VpnProfileDto, VpnProgressDto, VpnStatusDto } from '../types';

export interface VpnDeps extends StoreDeps {
  toast?: Pick<ToastStore, 'show'>;
}

export function createVpnStore(deps: VpnDeps = {}) {
  const { api, listen } = resolveDeps(deps);
  const toast = deps.toast ?? defaultToast;
  let status = $state<VpnStatusDto | null>(null);
  let progress = $state<VpnProgressDto | null>(null);
  let unlisten: Unlisten[] = [];

  async function refresh() {
    status = await api.vpnStatus();
  }

  function apply(next: VpnProgressDto) {
    if (next.phase === 'done' || next.phase === 'failed') {
      if (progress?.requestId === next.requestId) progress = null;
    } else if (next.phase !== 'testing') {
      progress = next;
    }
  }

  return {
    get status() {
      return status;
    },
    /** The connect whose VPN is coming up, for the progress dialog. */
    get progress() {
      return progress;
    },
    refresh,
    /** Whether RemoteDesktop holds a VPN for the entry (the sidebar badge). */
    holds(entryId: string): boolean {
      return status?.held.some((h) => h.entryIds.includes(entryId)) ?? false;
    },
    /** The VPN an external launch of the entry holds ("Release VPN"), or `null`. */
    externalProfile(entryId: string): string | null {
      return (
        status?.held.find((h) => h.externalEntries.some((e) => e.entryId === entryId))?.profileName ?? null
      );
    },
    profiles: (): Promise<VpnProfileDto[]> => api.vpnProfiles(),
    async pair() {
      await api.vpnPair();
      await refresh();
    },
    async unpair() {
      await api.vpnUnpair();
      await refresh();
    },
    /** Cancels the connect in the progress dialog (it then fails with `vpnCancelled`). */
    async cancel() {
      if (progress) await api.vpnCancel(progress.requestId);
    },
    releaseEntry: (entryId: string) => api.vpnReleaseEntry(entryId),
    releaseProfile: (profileId: string) => api.vpnReleaseProfile(profileId),
    async start() {
      if (unlisten.length > 0) return;
      unlisten = await Promise.all([
        onVpnProgress(listen, apply),
        onVpnChanged(listen, () => void refresh().catch(() => {})),
        onVpnDropped(listen, ({ profileName }) => toast.show(t('The VPN "%@" went down.', profileName))),
      ]);
      await refresh();
    },
    stop() {
      for (const off of unlisten) off();
      unlisten = [];
    },
  };
}

export type VpnStore = ReturnType<typeof createVpnStore>;

export const vpn = createVpnStore();
