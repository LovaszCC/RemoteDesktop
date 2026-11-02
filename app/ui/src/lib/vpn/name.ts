// The VPN app is MacVPN on macOS, WinVPN on Windows and LinVPN on Linux: same control API, same texts. The
// catalog says "MacVPN"; `vpnText` swaps the name in a translated string for this system.

import { app } from '../stores/app.svelte';
import type { Platform } from '../types';

export type VpnAppName = 'MacVPN' | 'WinVPN' | 'LinVPN';

export function vpnAppName(platform: Platform = app.platform): VpnAppName {
  switch (platform) {
    case 'windows':
      return 'WinVPN';
    case 'linux':
      return 'LinVPN';
    default:
      return 'MacVPN';
  }
}

/** `text` (already translated) with "MacVPN" replaced by this system's VPN app. */
export function vpnText(text: string, platform: Platform = app.platform): string {
  const name = vpnAppName(platform);
  return name === 'MacVPN' ? text : text.replaceAll('MacVPN', name);
}
