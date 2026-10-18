import { describe, expect, it, vi } from 'vitest';
import { connection } from '../testing/fixtures';
import type { ConnectionDto } from '../types';
import {
  MAX_PANES,
  addTerminalItems,
  broadcastTargets,
  commonColor,
  isAllBroadcasting,
  newSplit,
  savedSplitConnections,
  splitColumns,
  splitTitle,
  withAllBroadcastToggled,
  withBroadcast,
  withoutPane,
  withPane,
  type SplitState,
} from './split';

const pane = (id: string, connectionId = `c-${id}`) => ({ id, connectionId });

function split(ids: string[], broadcasting: string[] = []): SplitState {
  let state = newSplit(
    'T',
    ids.slice(0, 1).map((id) => pane(id)),
  );
  for (const id of ids.slice(1)) state = withPane(state, pane(id)) ?? state;
  for (const id of broadcasting) state = withBroadcast(state, id, true);
  return state;
}

describe('split layout', () => {
  it('lays out 1–4 panes in columns', () => {
    expect(splitColumns(0)).toEqual([]);
    expect(splitColumns(1)).toEqual([[0]]);
    expect(splitColumns(2)).toEqual([[0], [1]]);
    expect(splitColumns(3)).toEqual([[0], [1, 2]]);
    expect(splitColumns(4)).toEqual([
      [0, 2],
      [1, 3],
    ]);
  });

  it('clamps above the maximum and places every pane once', () => {
    expect(MAX_PANES).toBe(4);
    expect(splitColumns(7)).toEqual(splitColumns(4));
    for (let count = 1; count <= 4; count++) {
      expect(
        splitColumns(count)
          .flat()
          .sort((a, b) => a - b),
      ).toEqual([...Array(count).keys()]);
    }
  });
});

describe('broadcast group', () => {
  it('adds up to four panes, refuses duplicates and focuses the newest', () => {
    let state = newSplit('T', [pane('a')]);
    expect(state.focused).toBe('a');
    expect(withPane(state, pane('a'))).toBeNull();
    for (const id of ['b', 'c', 'd']) state = withPane(state, pane(id))!;
    expect(state.focused).toBe('d');
    expect(withPane(state, pane('e'))).toBeNull();
    expect(state.panes.map((p) => p.id)).toEqual(['a', 'b', 'c', 'd']);
    expect(state.broadcasting).toEqual([]);
    expect(
      newSplit(
        'T',
        ['a', 'b', 'c', 'd', 'e'].map((id) => pane(id)),
      ).panes,
    ).toHaveLength(4);
  });

  it('removing a pane drops its broadcast flag and moves the focus', () => {
    const state = withoutPane({ ...split(['a', 'b', 'c'], ['a', 'b']), focused: 'b' }, 'b');
    expect(state.panes.map((p) => p.id)).toEqual(['a', 'c']);
    expect(state.broadcasting).toEqual(['a']);
    expect(state.focused).toBe('c');
    expect(withoutPane(state, 'zzz')).toBe(state);
    expect(withoutPane(withoutPane(state, 'c'), 'a').focused).toBeNull();
  });

  it('ignores broadcast for an unknown pane', () => {
    expect(withBroadcast(split(['a']), 'x', true).broadcasting).toEqual([]);
  });

  it('has no targets while the source does not broadcast', () => {
    expect(broadcastTargets(split(['a', 'b', 'c'], ['b', 'c']), 'a', () => true)).toEqual([]);
  });

  it('targets the other broadcasting, running panes in pane order', () => {
    // d: syslog pane, not broadcasting; c: exited.
    const state = split(['a', 'b', 'c', 'd'], ['c', 'b', 'a']);
    expect(broadcastTargets(state, 'b', (id) => id !== 'c')).toEqual(['a']);
    expect(broadcastTargets(state, 'a', () => true)).toEqual(['b', 'c']);
  });

  it('toggles all and knows when all broadcast', () => {
    let state = split(['a', 'b'], ['a']);
    expect(isAllBroadcasting(state)).toBe(false);
    state = withAllBroadcastToggled(state);
    expect(isAllBroadcasting(state)).toBe(true);
    expect(broadcastTargets(state, 'b', () => true)).toEqual(['a']);
    state = withAllBroadcastToggled(state);
    expect(state.broadcasting).toEqual([]);
    expect(isAllBroadcasting({ ...state, panes: [] })).toBe(false);
  });
});

describe('split tab', () => {
  it('titles up to two panes by name, more by count', () => {
    expect(splitTitle(['a', 'b'])).toBe('a + b');
    expect(splitTitle(['a', 'b', 'c'])).toBe('Split (3)');
  });

  it('uses the colour every pane shares', () => {
    expect(commonColor(['red', 'red'])).toBe('red');
    expect(commonColor(['red', 'blue'])).toBeNull();
    expect(commonColor(['red', null])).toBeNull();
    expect(commonColor([])).toBeNull();
  });

  it('opens the saved connections that exist, in order, duplicates kept, at most four', () => {
    const all = new Map(['a', 'b'].map((id) => [id, connection(id)]));
    const ids = (list: string[]) => savedSplitConnections(list, (id) => all.get(id)).map((c) => c.id);
    expect(ids(['b', 'gone', 'a'])).toEqual(['b', 'a']);
    expect(ids(['a', 'a', 'b', 'b', 'a'])).toEqual(['a', 'a', 'b', 'b']);
    expect(ids(['gone'])).toEqual([]);
  });

  it('offers the panes again, then every SSH connection by name', () => {
    const conns: ConnectionDto[] = [
      { ...connection('z', 'zeta') },
      { ...connection('r', 'rdp'), type: 'rdp' },
      { ...connection('m', 'Mikro'), type: 'mikrotik' },
      { ...connection('a', 'alpha') },
    ];
    const add = vi.fn();
    const state = newSplit('T', [pane('p1', 'a'), pane('p2', 'z'), pane('p3', 'z')]);
    const items = addTerminalItems(state, conns, add);
    const [again, ssh] = items as { label: string; submenu: { label: string; action: () => void }[] }[];
    expect(again!.label).toBe('Again');
    expect(again!.submenu.map((i) => i.label)).toEqual(['alpha', 'zeta']);
    expect(ssh!.label).toBe('SSH connections');
    expect(ssh!.submenu.map((i) => i.label)).toEqual(['alpha', 'Mikro', 'zeta']);
    ssh!.submenu[1]!.action();
    expect(add).toHaveBeenCalledWith(conns[2]);
    expect(addTerminalItems(split(['x']), [], add)).toEqual([]);
    const addLocal = vi.fn();
    const withLocal = addTerminalItems(split(['x']), [], add, addLocal);
    expect(withLocal).toEqual([{ label: 'Local Terminal', action: addLocal }]);
  });
});
