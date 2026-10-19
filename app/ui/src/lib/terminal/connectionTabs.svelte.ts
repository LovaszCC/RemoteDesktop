// Which library connections have embedded sessions (Swift `AppState.isConnected`): the sidebar's green
// "open session" dot, and the tab a single click brings forward when Settings → Sidebar says so. Tabs are
// counted per entry: all tabs of an entry's connections belong to it.
//
// A session is live while its SSH process runs (a tab or a split pane) or its RDP connection isn't
// disconnected; an ended one (Reconnect bar) still has its tab but no dot.

import { rdpSessions as defaultRdp, type RdpSessionsStore } from '../rdp/sessions.svelte';
import {
  isEntryKind,
  library as defaultLibrary,
  parseTag,
  type LibraryStore,
} from '../stores/library.svelte';
import { tabs as defaultTabs, type TabsStore } from '../tabs.svelte';
import type { ConnectionType, SelectionShows } from '../types';
import { sessions as defaultSessions, type SessionsStore } from './sessions.svelte';
import { splits as defaultSplits, type SplitsStore } from './splits.svelte';

/** A tab an entry is open in, for the Details page's list. */
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
  /** Maps a connection to its entry; an unknown connection counts as its own entry. */
  library: Pick<LibraryStore, 'connection'>;
}

export function createConnectionTabs(deps: Partial<ConnectionTabsDeps> = {}) {
  const tabs = deps.tabs ?? defaultTabs;
  const sessions = deps.sessions ?? defaultSessions;
  const rdpSessions = deps.rdpSessions ?? defaultRdp;
  const splits = deps.splits ?? defaultSplits;
  const library = deps.library ?? defaultLibrary;

  /** The library connection of a plain (non-split) tab. */
  const connectionOf = (id: string): string | undefined =>
    sessions.has(id) && !sessions.isPane(id)
      ? sessions.state(id)?.connectionId
      : rdpSessions.state(id)?.connectionId;
  const entryOf = (connectionId: string): string => library.connection(connectionId)?.entryId ?? connectionId;
  const inEntry = (connectionId: string | undefined, entryId: string): boolean =>
    connectionId !== undefined && entryOf(connectionId) === entryId;

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
    /** The entry's own (non-split) tabs, of any of its connections, in tab order, live or ended. */
    plainTabs(entryId: string): string[] {
      return tabs.ids.filter((id) => inEntry(connectionOf(id), entryId));
    },
    /** Every tab the entry is open in, its own tabs and split views, in tab order. */
    openTabs(entryId: string): OpenTab[] {
      return tabs.ids.flatMap((id) => {
        const split = splits.get(id);
        const inSplit = split?.panes.some((p) => inEntry(p.connectionId, entryId)) ?? false;
        if (!inSplit && !inEntry(connectionOf(id), entryId)) return [];
        const session = tabs.session(id);
        return session ? [{ id, title: tabs.displayTitle(session), type: session.type, split: inSplit }] : [];
      });
    },
    /**
     * The tab a sidebar selection shows: with `tab`, the only plain tab of a single selected entry (`e:`,
     * `fav:`: of any of its connections) or child connection (`c:`: of that one); otherwise `null`
     * (Details). Several tabs, only split panes or a multi-selection show Details.
     */
    tabForSelection(selection: readonly string[], mode: SelectionShows): string | null {
      if (mode !== 'tab' || selection.length !== 1) return null;
      const tag = parseTag(selection[0] ?? '');
      let open: string[];
      if (tag?.kind === 'connection') open = tabs.ids.filter((id) => connectionOf(id) === tag.id);
      else if (isEntryKind(tag?.kind)) open = store.plainTabs(tag.id);
      else return null;
      return open.length === 1 ? (open[0] ?? null) : null;
    },
  };
  return store;
}

export type ConnectionTabs = ReturnType<typeof createConnectionTabs>;

/** The app-wide instance. */
export const connectionTabs = createConnectionTabs();
