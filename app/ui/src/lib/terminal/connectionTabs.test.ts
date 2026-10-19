import { describe, expect, it } from 'vitest';
import type { RdpSessionState } from '../rdp/model';
import { createConnectionTabs, type ConnectionTabsDeps } from './connectionTabs.svelte';
import type { TerminalSessionState } from './model';
import type { SplitState } from './split';

type Ssh = { connectionId: string; running?: boolean; pane?: boolean };
type Rdp = { connectionId: string; status?: RdpSessionState['status']['kind'] };

/** Tabs in order; `ssh`/`rdp` keyed by session id (a pane has no tab). */
function setup({
  ids = [],
  ssh = {},
  rdp = {},
  splits = {},
}: {
  ids?: string[];
  ssh?: Record<string, Ssh>;
  rdp?: Record<string, Rdp>;
  /** Split tab id → its panes' connection ids. */
  splits?: Record<string, string[]>;
}) {
  const sshStates = Object.fromEntries(
    Object.entries(ssh).map(([id, s]) => [
      id,
      { id, connectionId: s.connectionId, status: { kind: s.running === false ? 'exited' : 'running' } },
    ]),
  ) as unknown as Record<string, TerminalSessionState>;
  const rdpStates = Object.fromEntries(
    Object.entries(rdp).map(([id, s]) => [
      id,
      { id, connectionId: s.connectionId, status: { kind: s.status ?? 'connected' } },
    ]),
  ) as unknown as Record<string, RdpSessionState>;
  const deps: ConnectionTabsDeps = {
    tabs: {
      ids,
      session: (id) =>
        ids.includes(id) ? { id, title: `T-${id}`, type: rdp[id] ? 'rdp' : 'ssh', color: null } : undefined,
      displayTitle: (session) => session.title,
    },
    splits: {
      get: (id) =>
        id !== null && splits[id]
          ? ({
              id,
              panes: splits[id].map((connectionId, i) => ({ id: `${id}-${i}`, connectionId })),
            } as unknown as SplitState)
          : undefined,
    },
    sessions: {
      states: sshStates,
      has: (id) => id in sshStates,
      isPane: (id) => ssh[id]?.pane === true,
      state: (id) => (id === null ? undefined : sshStates[id]),
    },
    rdpSessions: {
      states: rdpStates,
      has: (id) => id in rdpStates,
      state: (id) => (id === null ? undefined : rdpStates[id]),
    },
  };
  return createConnectionTabs(deps);
}

describe('connectionTabs.isLive', () => {
  it('counts running SSH tabs and split panes and RDP sessions that are not disconnected', () => {
    const tabs = setup({
      ids: ['S1', 'R1'],
      ssh: { S1: { connectionId: 'a' }, P1: { connectionId: 'b', pane: true } },
      rdp: { R1: { connectionId: 'c', status: 'reconnecting' } },
    });
    expect(tabs.isLive('a')).toBe(true);
    expect(tabs.isLive('b')).toBe(true);
    expect(tabs.isLive('c')).toBe(true);
    expect(tabs.isLive('other')).toBe(false);
  });

  it('ignores ended sessions (Reconnect bar)', () => {
    const tabs = setup({
      ids: ['S1', 'R1'],
      ssh: { S1: { connectionId: 'a', running: false } },
      rdp: { R1: { connectionId: 'a', status: 'disconnected' } },
    });
    expect(tabs.isLive('a')).toBe(false);
  });
});

describe('connectionTabs.plainTabs', () => {
  it('lists the connection’s own tabs in tab order, live or ended, without split panes', () => {
    const tabs = setup({
      ids: ['R1', 'X', 'S2', 'SPLIT', 'S1'],
      ssh: {
        S1: { connectionId: 'a' },
        S2: { connectionId: 'a', running: false },
        P1: { connectionId: 'a', pane: true },
        X: { connectionId: 'b' },
      },
      rdp: { R1: { connectionId: 'a' } },
    });
    expect(tabs.plainTabs('a')).toEqual(['R1', 'S2', 'S1']);
    expect(tabs.plainTabs('none')).toEqual([]);
  });
});

describe('connectionTabs.tabForSelection', () => {
  const one = setup({
    ids: ['S1', 'S2', 'S3'],
    ssh: { S1: { connectionId: 'a' }, S2: { connectionId: 'b' }, S3: { connectionId: 'b' } },
  });

  it('shows the only tab of a single selected connection or favourite', () => {
    expect(one.tabForSelection(['c:a'], 'tab')).toBe('S1');
    expect(one.tabForSelection(['fav:a'], 'tab')).toBe('S1');
  });

  it('shows Details otherwise', () => {
    expect(one.tabForSelection(['c:a'], 'details')).toBeNull();
    expect(one.tabForSelection(['c:b'], 'tab')).toBeNull();
    expect(one.tabForSelection(['c:none'], 'tab')).toBeNull();
    expect(one.tabForSelection(['c:a', 'c:b'], 'tab')).toBeNull();
    expect(one.tabForSelection([], 'tab')).toBeNull();
    expect(one.tabForSelection(['f:a'], 'tab')).toBeNull();
    expect(one.tabForSelection(['k:a'], 'tab')).toBeNull();
  });

  it('shows Details when the connection is only open in a split view', () => {
    const split = setup({ ids: ['SPLIT'], ssh: { P1: { connectionId: 'a', pane: true } } });
    expect(split.tabForSelection(['c:a'], 'tab')).toBeNull();
  });
});

describe('connectionTabs.openTabs', () => {
  it('lists the connection’s own tabs and the split views it is in, in tab order', () => {
    const tabs = setup({
      ids: ['S1', 'SPLIT', 'X', 'R1', 'OTHER-SPLIT'],
      ssh: { S1: { connectionId: 'a' }, X: { connectionId: 'b' } },
      rdp: { R1: { connectionId: 'a', status: 'disconnected' } },
      splits: { SPLIT: ['b', 'a'], 'OTHER-SPLIT': ['b', 'c'] },
    });
    expect(tabs.openTabs('a')).toEqual([
      { id: 'S1', title: 'T-S1', type: 'ssh', split: false },
      { id: 'SPLIT', title: 'T-SPLIT', type: 'ssh', split: true },
      { id: 'R1', title: 'T-R1', type: 'rdp', split: false },
    ]);
    expect(tabs.openTabs('none')).toEqual([]);
  });
});
