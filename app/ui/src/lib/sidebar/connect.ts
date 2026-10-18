// Where Connect goes (Swift `AppState.connect` / `launchSSH`): SSH, and MikroTik in SSH mode, open an
// embedded session or the external terminal per the connection's (or the menu item's) choice; web opens
// the browser, MikroTik in WinBox mode starts WinBox, and RDP set to the system client hands a `.rdp` file
// to it; RDP set to the embedded client opens an RDP tab.

import type { ConnectionDto, MikrotikMode, RdpClient, SshTerminal } from '../types';

/** A menu item's overrides: the MikroTik mode, the SSH terminal and the RDP client. */
export interface ConnectOptions {
  mode?: MikrotikMode;
  terminal?: SshTerminal;
  rdpClient?: RdpClient;
}

export type ConnectRoute = 'embedded' | 'external' | 'web' | 'winbox' | 'rdpEmbedded' | 'rdpExternal';

export function connectRoute(
  connection: Pick<ConnectionDto, 'type' | 'mikrotikMode' | 'sshTerminal' | 'rdpClient'>,
  options: ConnectOptions = {},
): ConnectRoute {
  switch (connection.type) {
    case 'web':
      return 'web';
    case 'rdp':
      return (options.rdpClient ?? connection.rdpClient) === 'windowsApp' ? 'rdpExternal' : 'rdpEmbedded';
    case 'mikrotik':
      if ((options.mode ?? connection.mikrotikMode) === 'winbox') return 'winbox';
      break;
    case 'ssh':
      break;
  }
  return (options.terminal ?? connection.sshTerminal) === 'external' ? 'external' : 'embedded';
}
