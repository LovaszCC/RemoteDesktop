// Which library connections have embedded sessions (Swift `AppState.isConnected`): the sidebar's green
// "open session" dot, and the tab a single click brings forward when Settings → Sidebar says so.
//
// A session is live while its SSH process runs (a tab or a split pane) or its RDP connection isn't
// disconnected; an ended one (Reconnect bar) still has its tab but no dot.

import { rdpSessions as defaultRdp, type RdpSessionsStore } from '../rdp/sessions.svelte';
import { parseTag } from '../stores/library.svelte';
import { tabs as defaultTabs, type TabsStore } from '../tabs.svelte';
import type { ConnectionType, SelectionShows } from '../types';
import { sessions as defaultSessions, type SessionsStore } from './sessions.svelte';
import { splits as defaultSplits, type SplitsStore } from './splits.svelte';

/** A tab the connection is open in, for the Details page's list. */
export interface OpenTab {
  id: string;
  title: string;
  type: ConnectionType;
  split: boolean;
}

export interface ConnectionTabsDeps {
  tabs: Pick<TabsStore, 'ids' | 'session' | 'displayTitle'>;
  sessions: Pick<SessionsStore, 'states' | 'has' | 'isPane' | 'state'>;
  rdpSessions: Pick<RdpSessionsStore, 'states' | 'has' | 'state'>;
  splits: Pick<SplitsStore, 'get'>;
}

export function createConnectionTabs(deps: Partial<ConnectionTabsDeps> = {}) {
  const tabs = deps.tabs ?? defaultTabs;
  const sessions = deps.sessions ?? defaultSessions;
  const rdpSessions = deps.rdpSessions ?? defaultRdp;
  const splits = deps.splits ?? defaultSplits;

  const connectionOf = (id: string): string | undefined =>
    sessions.has(id) && !sessions.isPane(id)
      ? sessions.state(id)?.connectionId
      : rdpSessions.state(id)?.connectionId;

  const store = {
    /** Whether the connection has a live embedded session: an SSH tab or split pane, or an RDP tab. */
    isLive(connectionId: string): boolean {
      return (
        Object.values(sessions.states).some(
          (s) => s.connectionId === connectionId && s.status.kind === 'running',
        ) ||
        Object.values(rdpSessions.states).some(
          (s) => s.connectionId === connectionId && s.status.kind !== 'disconnected',
        )
      );
    },
    /** The connection's own (non-split) tabs, in tab order, live or ended. */
    plainTabs(connectionId: string): string[] {
      return tabs.ids.filter((id) => connectionOf(id) === connectionId);
    },
    /** Every tab the connection is open in, its own tabs and split views, in tab order. */
    openTabs(connectionId: string): OpenTab[] {
      return tabs.ids.flatMap((id) => {
        const split = splits.get(id);
        const inSplit = split?.panes.some((p) => p.connectionId === connectionId) ?? false;
        if (!inSplit && connectionOf(id) !== connectionId) return [];
        const session = tabs.session(id);
        return session ? [{ id, title: tabs.displayTitle(session), type: session.type, split: inSplit }] : [];
      });
    },
    /**
     * The tab a sidebar selection shows: with `tab`, the only plain tab of a single selected connection;
     * otherwise `null` (Details). Several tabs, only split panes or a multi-selection show Details.
     */
    tabForSelection(selection: readonly string[], mode: SelectionShows): string | null {
      if (mode !== 'tab' || selection.length !== 1) return null;
      const tag = parseTag(selection[0] ?? '');
      if (tag?.kind !== 'connection' && tag?.kind !== 'favorite') return null;
      const open = store.plainTabs(tag.id);
      return open.length === 1 ? (open[0] ?? null) : null;
    },
  };
  return store;
}

export type ConnectionTabs = ReturnType<typeof createConnectionTabs>;

/** The app-wide instance. */
export const connectionTabs = createConnectionTabs();
