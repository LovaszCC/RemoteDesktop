// Texts of the port forward UI that depend on Rust codes.

import { t } from '../i18n/index.svelte';
import type { ForwardStatusDto, PortForwardDto } from '../types';

/** Catalog keys for the tunnel failure codes (`port-forward-status` `code`). */
export const FAILURE_KEYS: Record<string, string> = {
  portInUse: 'Local port %@ is already in use.',
  timeout: "The tunnel didn't open within 30 seconds.",
  disconnected: 'The SSH connection was closed.',
  authFailed: 'Authentication failed',
  connectFailed: "Can't connect to the SSH server.",
  hostKeyRejected: 'The host key was not accepted.',
  hostKeyChanged: 'The host key has changed. Remove the old key from known_hosts if the change is expected.',
  hostKeyRevoked: 'The host key is revoked.',
  hostCertificate: 'Host certificates are not supported for port forwards.',
  knownHostsWrite: "Can't save the host key.",
  proxyUnsupported: 'ProxyJump and ProxyCommand are not supported for port forwards.',
};

/** The translated failure of a forward; the English `error` for unknown codes. */
export function failureText(forward: PortForwardDto, status: ForwardStatusDto): string {
  if (status.state !== 'failed') return '';
  const key = FAILURE_KEYS[status.code];
  if (!key) return status.error;
  return key.includes('%@') ? t(key, String(forward.localPort)) : t(key);
}

/** The editor's fields for a forward, or blank ones for a new forward of `connectionId`. */
export function draftOf(connectionId: string, forward?: PortForwardDto) {
  return {
    id: forward?.id ?? null,
    connectionId,
    name: forward?.name ?? '',
    localPort: forward ? String(forward.localPort) : '',
    remoteHost: forward?.remoteHost ?? '',
    remotePort: forward ? String(forward.remotePort) : '',
  };
}
