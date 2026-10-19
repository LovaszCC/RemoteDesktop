// The sidebar's pure logic (Swift `SidebarView`, `SidebarDrag`, the `AppState` selection helpers): which
// sections and rows show, the visible row order for Shift-click ranges and the arrow keys, drag payloads
// and the texts of the delete confirmations. Components only render what these return.

import { t, tn } from '../i18n/index.svelte';
import { unique } from '../collections';
import { connectionOfTag, isEntryKind, parseTag } from '../stores/library.svelte';
import type {
  Platform,
  ConnectionDto,
  ConnectionType,
  EntryDto,
  FolderDto,
  LabelColor,
  LibraryDto,
  SearchResultDto,
  TreeNodeDto,
} from '../types';

export type SectionId = 'results' | 'favorites' | 'splits' | 'connections' | 'credentials';

export type SidebarRow =
  /** An entry (`e:` in the tree and results, `fav:` under Favorites); 2+ connections expand in the tree. */
  | {
      kind: 'entry';
      tag: string;
      id: string;
      depth: number;
      subtitle: string | null;
      expanded: boolean;
      hasChildren: boolean;
    }
  /** A connection of an expanded entry (`c:`): its display label, the default marked. */
  | { kind: 'connection'; tag: string; id: string; depth: number; label: string; isDefault: boolean }
  | { kind: 'folder'; tag: string; id: string; depth: number; expanded: boolean; hasChildren: boolean }
  | { kind: 'credential'; tag: string; id: string; depth: 0 }
  | { kind: 'split'; tag: string; id: string; depth: 0 };

/** Where a dragged item would land on a row. */
export type DropMark = 'into' | 'before' | 'after' | null;

export interface SidebarSection {
  id: SectionId;
  /** Catalog key of the header. */
  title: string;
  rows: SidebarRow[];
}

export interface SectionInput {
  library: LibraryDto;
  /** The search text; not blank means search mode. */
  query: string;
  /** Search hits (`null` while the first search is running). */
  results: SearchResultDto | null;
  expanded: ReadonlySet<string>;
}

/** The connection types in menu order (Swift `ConnectionType.allCases`). */
export const CONNECTION_TYPES: readonly ConnectionType[] = ['rdp', 'ssh', 'web', 'mikrotik'];

/** Catalog keys of the "New … connection" titles. */
export const NEW_TITLES: Record<ConnectionType, string> = {
  rdp: 'New RDP connection',
  ssh: 'New SSH connection',
  web: 'New web connection',
  mikrotik: 'New MikroTik connection',
};

/** Swift `ConnectionType.displayName`. */
export function typeName(type: ConnectionType): string {
  switch (type) {
    case 'rdp':
      return 'RDP';
    case 'ssh':
      return 'SSH';
    case 'web':
      return t('Web');
    case 'mikrotik':
      return 'MikroTik';
  }
}

/** SSH and MikroTik open over SSH (Swift `usesSSH`). */
export function usesSSH(type: ConnectionType): boolean {
  return type === 'ssh' || type === 'mikrotik';
}

/** Swift `LabelColor.title`, translated. */
export function colorTitle(color: LabelColor): string {
  return t(color.charAt(0).toUpperCase() + color.slice(1));
}

/** The row tooltip: "SSH · root@host", without `user@` when there is no user. */
export function connectionTooltip(connection: ConnectionDto): string {
  const user = connection.username ? `${connection.username}@` : '';
  return `${typeName(connection.type)} · ${user}${connection.host}`;
}

/** The saved split caption: its existing connections' names joined with " + ", or "no connections". */
export function splitCaption(names: readonly string[]): string {
  return names.length === 0 ? t('no connections') : names.join(' + ');
}

/** An entry row; only the tree (`expanded` given) lets an entry with 2+ connections expand. */
function entryRow(
  entry: EntryDto,
  prefix: 'e:' | 'fav:',
  depth: number,
  subtitle: string | null,
  expanded?: ReadonlySet<string>,
): SidebarRow {
  const hasChildren = expanded !== undefined && entry.connectionIds.length > 1;
  return {
    kind: 'entry',
    tag: prefix + entry.id,
    id: entry.id,
    depth,
    subtitle,
    expanded: hasChildren && expanded.has(entry.id),
    hasChildren,
  };
}

function treeRows(
  nodes: readonly TreeNodeDto[],
  library: LibraryDto,
  entries: ReadonlyMap<string, EntryDto>,
  expanded: ReadonlySet<string>,
  depth: number,
): SidebarRow[] {
  const rows: SidebarRow[] = [];
  for (const node of nodes) {
    if (node.kind === 'entry') {
      const entry = entries.get(node.id);
      if (!entry) continue;
      const row = entryRow(entry, 'e:', depth, null, expanded);
      rows.push(row);
      if (row.kind === 'entry' && row.expanded) rows.push(...childRows(library, entry, depth + 1));
      continue;
    }
    const open = expanded.has(node.id);
    rows.push({
      kind: 'folder',
      tag: `f:${node.id}`,
      id: node.id,
      depth,
      expanded: open,
      hasChildren: node.children.length > 0,
    });
    if (open) rows.push(...treeRows(node.children, library, entries, expanded, depth + 1));
  }
  return rows;
}

/** The child rows of an expanded entry, in its connection order. */
function childRows(library: LibraryDto, entry: EntryDto, depth: number): SidebarRow[] {
  return entry.connectionIds.flatMap((id) => {
    const connection = library.connections.find((c) => c.id === id);
    if (!connection) return [];
    return [
      {
        kind: 'connection' as const,
        tag: `c:${id}`,
        id,
        depth,
        label: connection.displayLabel,
        isDefault: id === entry.defaultConnectionId,
      },
    ];
  });
}

const credentialRow = (id: string): SidebarRow => ({ kind: 'credential', tag: `k:${id}`, id, depth: 0 });

/**
 * The sections in display order. Search mode: "Results" (folder path subtitles) and "Credentials" when
 * any matches. Normal mode: "Favorites" and "Saved splits" when not empty, then "Connections" (the tree,
 * collapsed folders hide their children) and "Credentials" (always).
 */
export function buildSections({ library, query, results, expanded }: SectionInput): SidebarSection[] {
  const entries = new Map(library.entries.map((e) => [e.id, e]));
  const rowsOf = (ids: readonly string[], prefix: 'e:' | 'fav:', subtitles: boolean) =>
    ids.flatMap((id) => {
      const entry = entries.get(id);
      return entry ? [entryRow(entry, prefix, 0, subtitles ? entry.folderPath : null)] : [];
    });
  if (query !== '') {
    const hits =
      query.trim() === '' ? { entries: [], credentials: [] } : (results ?? { entries: [], credentials: [] });
    const sections: SidebarSection[] = [
      { id: 'results', title: 'Results', rows: rowsOf(hits.entries, 'e:', true) },
    ];
    if (hits.credentials.length > 0) {
      sections.push({ id: 'credentials', title: 'Credentials', rows: hits.credentials.map(credentialRow) });
    }
    return sections;
  }
  const sections: SidebarSection[] = [];
  if (library.favorites.length > 0) {
    sections.push({ id: 'favorites', title: 'Favorites', rows: rowsOf(library.favorites, 'fav:', false) });
  }
  if (library.savedSplits.length > 0) {
    sections.push({
      id: 'splits',
      title: 'Saved splits',
      rows: library.savedSplits.map((s) => ({ kind: 'split', tag: `s:${s.id}`, id: s.id, depth: 0 })),
    });
  }
  sections.push({
    id: 'connections',
    title: 'Connections',
    rows: treeRows(library.tree, library, entries, expanded, 0),
  });
  sections.push({
    id: 'credentials',
    title: 'Credentials',
    rows: library.credentials.map((c) => credentialRow(c.id)),
  });
  return sections;
}

/** Every visible row's tag top to bottom: the order of Shift-click ranges and the arrow keys. */
export function visibleOrder(sections: readonly SidebarSection[]): string[] {
  return sections.flatMap((s) => s.rows.map((r) => r.tag));
}

/** The library has nothing at all and no search runs: the sidebar shows the empty state. */
export function isLibraryEmpty(library: LibraryDto, query: string): boolean {
  return (
    query === '' &&
    library.folders.length === 0 &&
    library.entries.length === 0 &&
    library.credentials.length === 0
  );
}

/** The tag `delta` rows away from `current` (the first/last row when nothing is selected). */
export function stepSelection(
  order: readonly string[],
  current: string | null,
  delta: number,
): string | null {
  if (order.length === 0) return null;
  const at = current ? order.indexOf(current) : -1;
  if (at < 0) return (delta > 0 ? order[0] : order[order.length - 1]) ?? null;
  return order[Math.min(order.length - 1, Math.max(0, at + delta))] ?? null;
}

/** Expanded folder and entry ids as the `expandedFolders` setting stores them: sorted. */
export function toggledExpansion(expanded: ReadonlySet<string>, folderId: string, open?: boolean): string[] {
  const next = new Set(expanded);
  const shouldOpen = open ?? !next.has(folderId);
  if (shouldOpen) next.add(folderId);
  else next.delete(folderId);
  return [...next].sort();
}

// Drag and drop (Swift `SidebarDrag`)

/** The drag data type of sidebar tags; the payload is the tags joined with "\n". */
export const DRAG_TAGS = 'application/x-rd-tags';
/** The drag data type of a saved split being reordered; the payload is its id. */
export const DRAG_SPLIT = 'application/x-rd-split';

/** What a dragged row carries: the whole selection (sorted) when the row is part of it, else the row. */
export function dragPayload(tag: string, selection: readonly string[]): string {
  if (selection.length <= 1 || !selection.includes(tag)) return tag;
  return [...selection].sort().join('\n');
}

/** The tags inside drag payloads. */
export function payloadTags(payload: string): string[] {
  return payload.split('\n').filter((tag) => tag !== '');
}

/** Which rows can be dragged into folders: folders and entries (tree, favourites, results). */
export function isDraggable(row: SidebarRow): boolean {
  return row.kind === 'folder' || row.kind === 'entry';
}

/** The saved split ids after moving `moved` before (or after) `target`. */
export function reorderedIds(
  ids: readonly string[],
  moved: string,
  target: string,
  after: boolean,
): string[] {
  if (moved === target || !ids.includes(moved) || !ids.includes(target)) return [...ids];
  const rest = ids.filter((id) => id !== moved);
  const at = rest.indexOf(target) + (after ? 1 : 0);
  return [...rest.slice(0, at), moved, ...rest.slice(at)];
}

// Selection helpers

/** The ids behind tags, de-duplicated; a favourite row is its entry (Swift `SidebarDrag.items`). */
export function tagItems(tags: readonly string[]) {
  const entries: string[] = [];
  const connections: string[] = [];
  const folders: string[] = [];
  const credentials: string[] = [];
  const splits: string[] = [];
  for (const tag of tags) {
    const parsed = parseTag(tag);
    if (!parsed) continue;
    if (isEntryKind(parsed.kind)) entries.push(parsed.id);
    else if (parsed.kind === 'connection') connections.push(parsed.id);
    else if (parsed.kind === 'folder') folders.push(parsed.id);
    else if (parsed.kind === 'credential') credentials.push(parsed.id);
    else splits.push(parsed.id);
  }
  return {
    entryIds: unique(entries),
    connectionIds: unique(connections),
    folderIds: unique(folders),
    credentialIds: unique(credentials),
    splitIds: unique(splits),
  };
}

/** The tags a context menu acts on: the selection when the clicked row is part of it, else that row. */
export function menuTags(clicked: string | null, selection: readonly string[]): string[] {
  if (clicked === null) return [];
  return selection.includes(clicked) ? [...selection] : [clicked];
}

/**
 * The SSH-capable connections of `tags` in library order (Swift `sshConnections(forTags:)`): a child row's
 * own, an entry's default when it uses SSH, else the entry's first that does.
 */
export function sshConnections(library: LibraryDto, tags: readonly string[]): ConnectionDto[] {
  const byId = new Map(library.connections.map((c) => [c.id, c]));
  const ids = new Set<string>();
  for (const tag of tags) {
    const parsed = parseTag(tag);
    if (parsed?.kind === 'connection') {
      if (byId.get(parsed.id) && usesSSH(byId.get(parsed.id)!.type)) ids.add(parsed.id);
    } else if (isEntryKind(parsed?.kind)) {
      const entry = library.entries.find((e) => e.id === parsed!.id);
      if (!entry) continue;
      const candidates = [entry.defaultConnectionId, ...entry.connectionIds];
      const id = candidates.find((c) => c !== null && byId.has(c) && usesSSH(byId.get(c)!.type));
      if (id) ids.add(id);
    }
  }
  return library.connections.filter((c) => ids.has(c.id));
}

/** `folderId` and every folder below it. */
export function subtreeFolderIds(folders: readonly FolderDto[], folderId: string): Set<string> {
  const result = new Set([folderId]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const folder of folders) {
      if (folder.parentId && result.has(folder.parentId) && !result.has(folder.id)) {
        result.add(folder.id);
        grew = true;
      }
    }
  }
  return result;
}

/**
 * The connections a "Linked credential" choice changes: the selected ones (an entry's default) plus those
 * in the selected folders' subtrees, without repeats (Swift `Library.connectionIds(forTags:)`).
 */
export function connectionIdsForTags(library: LibraryDto, tags: readonly string[]): string[] {
  const items = tagItems(tags);
  const folders = new Set<string>();
  for (const id of items.folderIds) for (const f of subtreeFolderIds(library.folders, id)) folders.add(f);
  const inFolders = library.connections.filter((c) => c.folderId && folders.has(c.folderId)).map((c) => c.id);
  const picked = tags.flatMap((tag) => {
    const parsed = parseTag(tag);
    if (parsed?.kind !== 'connection' && !isEntryKind(parsed?.kind)) return [];
    return connectionOfTag(library, tag)?.id ?? [];
  });
  return unique([...picked, ...inFolders]);
}

// Confirmation texts

export interface DeletionCounts {
  connections: number;
  folders: number;
  credentials: number;
}

/**
 * The items a bulk delete removes (saved splits are not part of it, as in Swift): an entry row deletes
 * the entry with its connections, a child row only that connection.
 */
export function deletionCounts(tags: readonly string[]): DeletionCounts & { tags: string[] } {
  const items = tagItems(tags);
  return {
    connections: items.entryIds.length + items.connectionIds.length,
    folders: items.folderIds.length,
    credentials: items.credentialIds.length,
    tags: [
      ...items.entryIds.map((id) => `e:${id}`),
      ...items.connectionIds.map((id) => `c:${id}`),
      ...items.folderIds.map((id) => `f:${id}`),
      ...items.credentialIds.map((id) => `k:${id}`),
    ],
  };
}

export function bulkDeleteTitle(counts: DeletionCounts): string {
  return t('Delete %lld items?', counts.connections + counts.folders + counts.credentials);
}

/** The bulk delete message, one line per fact (Swift `bulkDeletionMessage`). */
export function bulkDeleteMessage(counts: DeletionCounts): string {
  const lines = [
    t(
      'Connections: %lld, folders: %lld, credentials: %lld.',
      counts.connections,
      counts.folders,
      counts.credentials,
    ),
  ];
  if (counts.folders > 0) {
    lines.push(
      t('Subfolders are deleted too; connections inside the folders are kept and move to Connections.'),
    );
  }
  if (counts.credentials > 0) {
    lines.push(t('Connections using a deleted credential fall back to their own saved login.'));
  }
  lines.push(t("This can't be undone."));
  return lines.join('\n');
}

/** The message of "Delete credential?" for a credential `count` connections use. */
export function credentialDeleteMessage(count: number, name: string): string {
  return tn(
    '%lld connections use the credential “%@”. After deleting it they use their own saved login.',
    count,
    name,
  );
}

// Texts

/** The external terminal's name for "Open in …" (the choice arrives with Phase 4). */
export function terminalName(platform: Platform): string {
  return platform === 'windows' ? 'Windows Terminal' : 'Terminal';
}

/**
 * The empty state's hint with the effective shortcuts of New RDP, New SSH and Import; the Swift text
 * while the shortcuts aren't loaded or one is removed.
 */
export function emptyHint(displays: readonly (string | null)[]): string {
  if (displays.length === 3 && displays.every((d) => d)) {
    return t('%@ new RDP, %@ new SSH connection, %@ import', ...displays);
  }
  return t('⌘N new RDP, ⇧⌘N new SSH connection, ⌘I import');
}
