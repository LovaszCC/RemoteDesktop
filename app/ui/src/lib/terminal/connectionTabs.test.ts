import { describe, expect, it } from 'vitest';
import type { RdpSessionState } from '../rdp/model';
import { createConnectionTabs, type ConnectionTabsDeps } from './connectionTabs.svelte';
import type { TerminalSessionState } from './model';

type Ssh = { connectionId: string; running?: boolean; pane?: boolean };
type Rdp = { connectionId: string; status?: RdpSessionState['status']['kind'] };

/** Tabs in order; `ssh`/`rdp` keyed by session id (a pane has no tab). */
function setup({
  ids = [],
  ssh = {},
  rdp = {},
}: {
  ids?: string[];
  ssh?: Record<string, Ssh>;
  rdp?: Record<string, Rdp>;
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
    tabs: { ids },
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
