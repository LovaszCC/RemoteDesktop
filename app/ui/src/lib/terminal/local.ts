// The local terminal (the sidebar's fixed "Local Terminal" entry): this machine's shell as an embedded
// session. It has no connection in the library; its sessions carry `LOCAL_TERMINAL_ID` as their connection
// id, so tabs, split panes, the exit bar and reconnect treat it like an SSH session.

import { t } from '../i18n/index.svelte';
import type { ConnectionDto } from '../types';

/** The connection id of local terminal sessions (never a library id: those are UUIDs). */
export const LOCAL_TERMINAL_ID = 'local-terminal';

export function isLocalTerminal(connectionId: string): boolean {
  return connectionId === LOCAL_TERMINAL_ID;
}

/** What opens a local terminal as a session or split pane (the name is chosen when it opens). */
export const LOCAL_TERMINAL: Pick<ConnectionDto, 'id' | 'name' | 'type' | 'color'> = {
  id: LOCAL_TERMINAL_ID,
  name: 'Local Terminal',
  type: 'ssh',
  color: null,
};

/** The tab title of a new local terminal: "Local Terminal", then "Local Terminal 2", 3… (the lowest free). */
export function localTerminalName(taken: readonly string[]): string {
  const base = t('Local Terminal');
  for (let n = 1; ; n++) {
    const name = n === 1 ? base : `${base} ${n}`;
    if (!taken.includes(name)) return name;
  }
}
