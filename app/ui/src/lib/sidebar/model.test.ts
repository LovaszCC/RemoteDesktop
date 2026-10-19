import { afterEach, describe, expect, it } from 'vitest';
import { setLocale } from '../i18n/index.svelte';
import { connection, multiEntryLibrary, sidebarLibrary } from '../testing/fixtures';
import { EMPTY_LIBRARY } from '../types';
import {
  buildSections,
  bulkDeleteMessage,
  bulkDeleteTitle,
  connectionIdsForTags,
  connectionTooltip,
  credentialDeleteMessage,
  deletionCounts,
  dragPayload,
  emptyHint,
  isDraggable,
  isLibraryEmpty,
  menuTags,
  payloadTags,
  reorderedIds,
  splitCaption,
  sshConnections,
  stepSelection,
  subtreeFolderIds,
  tagItems,
  toggledExpansion,
  visibleOrder,
} from './model';

afterEach(() => setLocale('en'));

const normal = (expanded: string[] = []) =>
  buildSections({ library: sidebarLibrary(), query: '', results: null, expanded: new Set(expanded) });

describe('sidebar sections', () => {
  it('lists Favorites, Saved splits, Connections and Credentials in order', () => {
    const sections = normal();
    expect(sections.map((s) => s.title)).toEqual(['Favorites', 'Saved splits', 'Connections', 'Credentials']);
    expect(visibleOrder(sections)).toEqual([
      'fav:c1',
      'fav:c2',
      's:s1',
      's:s2',
      'f:f1',
      'f:f3',
      'e:c1',
      'e:c5',
      'e:c4',
      'k:k1',
      'k:k2',
    ]);
  });

  it('leaves out empty Favorites and Saved splits but always shows Credentials', () => {
    const library = { ...sidebarLibrary(), favorites: [], savedSplits: [], credentials: [] };
    const sections = buildSections({ library, query: '', results: null, expanded: new Set() });
    expect(sections.map((s) => s.id)).toEqual(['connections', 'credentials']);
    expect(sections[1]!.rows).toEqual([]);
  });

  it('shows the children of expanded folders, indented', () => {
    const tree = normal(['f1', 'f2']).find((s) => s.id === 'connections')!.rows;
    expect(tree.map((r) => [r.tag, r.depth])).toEqual([
      ['f:f1', 0],
      ['f:f2', 1],
      ['e:c3', 2],
      ['e:c2', 1],
      ['f:f3', 0],
      ['e:c1', 0],
      ['e:c5', 0],
      ['e:c4', 0],
    ]);
    const office = tree[0]!;
    expect(office.kind === 'folder' && office.expanded && office.hasChildren).toBe(true);
    const lab = tree.find((r) => r.tag === 'f:f3')!;
    expect(lab.kind === 'folder' && lab.hasChildren).toBe(false);
    // A collapsed parent hides an expanded child.
    expect(visibleOrder(normal(['f2']))).not.toContain('e:c3');
  });

  it('search mode shows Results with folder paths and Credentials only when any match', () => {
    const library = sidebarLibrary();
    const results = { entries: ['c3', 'missing', 'c1'], credentials: [] };
    let sections = buildSections({ library, query: 'x', results, expanded: new Set() });
    expect(sections.map((s) => s.title)).toEqual(['Results']);
    expect(sections[0]!.rows.map((r) => r.tag)).toEqual(['e:c3', 'e:c1']);
    expect(sections[0]!.rows[0]).toMatchObject({ kind: 'entry', subtitle: 'Office / Servers' });
    sections = buildSections({
      library,
      query: 'x',
      results: { entries: [], credentials: ['k2'] },
      expanded: new Set(),
    });
    expect(sections.map((s) => s.title)).toEqual(['Results', 'Credentials']);
    expect(visibleOrder(sections)).toEqual(['k:k2']);
  });

  it('a query of spaces gives an empty Results section', () => {
    const sections = buildSections({
      library: sidebarLibrary(),
      query: '   ',
      results: { entries: ['c1'], credentials: ['k1'] },
      expanded: new Set(),
    });
    expect(sections).toEqual([{ id: 'results', title: 'Results', rows: [] }]);
  });

  it('entry_rows_expand_to_their_connections', () => {
    const rows = (expanded: string[]) =>
      buildSections({
        library: multiEntryLibrary(),
        query: '',
        results: null,
        expanded: new Set(expanded),
      }).find((s) => s.id === 'connections')!.rows;
    const collapsed = rows([]).find((r) => r.tag === 'e:e1')!;
    expect(collapsed).toMatchObject({ kind: 'entry', expanded: false, hasChildren: true });
    expect(rows([]).map((r) => r.tag)).not.toContain('c:h1');
    const open = rows(['e1']);
    const at = open.findIndex((r) => r.tag === 'e:e1');
    expect(open[at]).toMatchObject({ kind: 'entry', expanded: true });
    expect(open.slice(at + 1, at + 3)).toEqual([
      { kind: 'connection', tag: 'c:h1', id: 'h1', depth: 1, label: 'SSH · root', isDefault: true },
      { kind: 'connection', tag: 'c:h2', id: 'h2', depth: 1, label: 'Console', isDefault: false },
    ]);
    expect(open[at + 3]?.tag).toBe('e:c5');
  });

  it('single_connection_entry_has_no_chevron', () => {
    const rows = normal(['c1']).find((s) => s.id === 'connections')!.rows;
    expect(rows.find((r) => r.tag === 'e:c1')).toMatchObject({
      kind: 'entry',
      hasChildren: false,
      expanded: false,
    });
    expect(rows.map((r) => r.tag)).not.toContain('c:c1');
  });

  it('search_results_are_entry_rows', () => {
    const sections = buildSections({
      library: multiEntryLibrary(),
      query: 'root',
      results: { entries: ['e1'], credentials: [] },
      expanded: new Set(['e1']),
    });
    // Results never expand: a child row would repeat the tree's `c:` tag.
    expect(sections[0]!.rows).toEqual([
      {
        kind: 'entry',
        tag: 'e:e1',
        id: 'e1',
        depth: 0,
        subtitle: '',
        expanded: false,
        hasChildren: false,
      },
    ]);
  });

  it('a favourite entry shows once under Favorites', () => {
    const library = { ...multiEntryLibrary(), favorites: ['c1', 'e1'] };
    const favorites = buildSections({ library, query: '', results: null, expanded: new Set(['e1']) })[0]!;
    expect(favorites.rows.map((r) => r.tag)).toEqual(['fav:c1', 'fav:e1']);
  });

  it('knows when the library is empty', () => {
    expect(isLibraryEmpty(EMPTY_LIBRARY, '')).toBe(true);
    expect(isLibraryEmpty(EMPTY_LIBRARY, 'a')).toBe(false);
    expect(isLibraryEmpty(sidebarLibrary(), '')).toBe(false);
  });
});

describe('sidebar rows', () => {
  it('tooltips leave out user@ without a user', () => {
    expect(connectionTooltip({ ...connection('a'), host: 'h', username: '' })).toBe('SSH · h');
    expect(connectionTooltip({ ...connection('a'), type: 'rdp', host: 'h', username: 'u' })).toBe(
      'RDP · u@h',
    );
    expect(connectionTooltip({ ...connection('a'), type: 'mikrotik', host: 'h' })).toBe('MikroTik · h');
  });

  it('split captions join names or say no connections', () => {
    expect(splitCaption(['a', 'b'])).toBe('a + b');
    expect(splitCaption([])).toBe('no connections');
  });

  it('builds the empty hint from the effective shortcuts', () => {
    expect(emptyHint(['Ctrl+Shift+R', 'Ctrl+Shift+S', 'Ctrl+Shift+I'])).toBe(
      'Ctrl+Shift+R new RDP, Ctrl+Shift+S new SSH connection, Ctrl+Shift+I import',
    );
    expect(emptyHint([null, '⇧⌘N', '⌘I'])).toBe('⌘N new RDP, ⇧⌘N new SSH connection, ⌘I import');
  });
});

describe('keyboard and expansion', () => {
  it('steps through the visible order and stops at the ends', () => {
    const order = ['a', 'b', 'c'];
    expect(stepSelection(order, null, 1)).toBe('a');
    expect(stepSelection(order, null, -1)).toBe('c');
    expect(stepSelection(order, 'b', 1)).toBe('c');
    expect(stepSelection(order, 'c', 1)).toBe('c');
    expect(stepSelection(order, 'a', -1)).toBe('a');
    expect(stepSelection([], 'a', 1)).toBeNull();
  });

  it('toggles expansion into a sorted list', () => {
    expect(toggledExpansion(new Set(['b']), 'a')).toEqual(['a', 'b']);
    expect(toggledExpansion(new Set(['a', 'b']), 'a')).toEqual(['b']);
    expect(toggledExpansion(new Set(['a']), 'a', true)).toEqual(['a']);
    expect(toggledExpansion(new Set(['a']), 'b', false)).toEqual(['a']);
  });
});

describe('drag and drop', () => {
  it('carries the whole selection only when the row is part of a multi-selection', () => {
    expect(dragPayload('c:1', [])).toBe('c:1');
    expect(dragPayload('c:1', ['c:1'])).toBe('c:1');
    expect(dragPayload('c:1', ['f:2', 'c:3'])).toBe('c:1');
    expect(dragPayload('c:1', ['f:2', 'c:1', 'fav:3'])).toBe('c:1\nf:2\nfav:3');
    expect(payloadTags('c:1\nf:2\n')).toEqual(['c:1', 'f:2']);
  });

  it('drags folders and entries only', () => {
    const rows = visibleOrder(normal());
    const sections = normal().flatMap((s) => s.rows);
    expect(sections.filter(isDraggable).map((r) => r.tag)).toEqual(
      rows.filter((tag) => /^(f|e|fav):/.test(tag)),
    );
  });

  it('reorders saved splits before or after the target', () => {
    expect(reorderedIds(['a', 'b', 'c'], 'c', 'a', false)).toEqual(['c', 'a', 'b']);
    expect(reorderedIds(['a', 'b', 'c'], 'a', 'c', true)).toEqual(['b', 'c', 'a']);
    expect(reorderedIds(['a', 'b', 'c'], 'a', 'b', true)).toEqual(['b', 'a', 'c']);
    expect(reorderedIds(['a', 'b'], 'a', 'a', true)).toEqual(['a', 'b']);
    expect(reorderedIds(['a', 'b'], 'x', 'a', true)).toEqual(['a', 'b']);
  });
});

describe('selection helpers', () => {
  it('maps tags to ids without repeats (a favourite is its entry)', () => {
    expect(tagItems(['e:1', 'fav:1', 'c:9', 'f:2', 'k:3', 's:4', 'x:5'])).toEqual({
      entryIds: ['1'],
      connectionIds: ['9'],
      folderIds: ['2'],
      credentialIds: ['3'],
      splitIds: ['4'],
    });
  });

  it('acts on the selection only when the clicked row is in it', () => {
    expect(menuTags('c:1', ['c:1', 'c:2'])).toEqual(['c:1', 'c:2']);
    expect(menuTags('c:3', ['c:1', 'c:2'])).toEqual(['c:3']);
    expect(menuTags(null, ['c:1'])).toEqual([]);
  });

  it('finds SSH-capable connections in library order', () => {
    const library = sidebarLibrary();
    expect(sshConnections(library, ['e:c4', 'fav:c1', 'e:c2', 'e:c5']).map((c) => c.id)).toEqual([
      'c1',
      'c4',
    ]);
  });

  it('an entry in a split uses its default SSH connection, else its first SSH one', () => {
    const library = multiEntryLibrary();
    expect(sshConnections(library, ['e:e1']).map((c) => c.id)).toEqual(['h1']);
    const rdpDefault = {
      ...library,
      entries: library.entries.map((e) => (e.id === 'e1' ? { ...e, defaultConnectionId: 'h2' } : e)),
    };
    expect(sshConnections(rdpDefault, ['e:e1']).map((c) => c.id)).toEqual(['h1']);
    expect(sshConnections(library, ['c:h2'])).toEqual([]);
  });

  it('collects connections of selected folders with their subtrees', () => {
    const library = sidebarLibrary();
    expect([...subtreeFolderIds(library.folders, 'f1')].sort()).toEqual(['f1', 'f2']);
    expect(connectionIdsForTags(library, ['f:f1', 'e:c1', 'fav:c1']).sort()).toEqual(['c1', 'c2', 'c3']);
    expect(connectionIdsForTags(multiEntryLibrary(), ['e:e1', 'c:h2'])).toEqual(['h1', 'h2']);
    expect(connectionIdsForTags(library, ['f:f3', 'k:k1'])).toEqual([]);
  });
});

describe('confirmation texts', () => {
  it('builds the bulk delete title and lines', () => {
    const counts = deletionCounts(['e:1', 'fav:1', 'c:2', 'f:3', 'k:4', 's:5'], sidebarLibrary());
    expect(counts.tags).toEqual(['e:1', 'c:2', 'f:3', 'k:4']);
    expect(bulkDeleteTitle(counts)).toBe('Delete 4 items?');
    expect(bulkDeleteMessage(counts)).toBe(
      [
        'Connections: 2, folders: 1, credentials: 1.',
        'Subfolders are deleted too; connections inside the folders are kept and move to Connections.',
        'Connections using a deleted credential fall back to their own saved login.',
        "This can't be undone.",
      ].join('\n'),
    );
    expect(bulkDeleteMessage(deletionCounts(['e:1'], sidebarLibrary()))).toBe(
      "Connections: 1, folders: 0, credentials: 0.\nThis can't be undone.",
    );
  });

  it('counts every connection of a deleted entry once', () => {
    const counts = deletionCounts(['e:e1', 'c:h2', 'c:c1'], multiEntryLibrary());
    expect([counts.items, counts.connections]).toEqual([3, 3]);
    expect(deletionCounts(['c:h2'], multiEntryLibrary()).connections).toBe(1);
  });

  it('builds the credential delete message with plurals', () => {
    expect(credentialDeleteMessage(3, 'Admin')).toBe(
      '3 connections use the credential “Admin”. After deleting it they use their own saved login.',
    );
    expect(credentialDeleteMessage(1, 'Admin')).toBe(
      '1 connection uses the credential “Admin”. After deleting it, it uses its own saved login.',
    );
    setLocale('hu');
    expect(credentialDeleteMessage(2, 'Admin')).toContain('Admin');
  });
});
