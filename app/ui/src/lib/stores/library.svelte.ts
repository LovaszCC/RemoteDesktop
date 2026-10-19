// The library as Rust sends it (`library-changed` replaces it whole), the sidebar selection and the
// search. Selection entries are sidebar tags: `f:<folder>`, `e:<entry>`, `c:<connection>` (a child row of
// an entry), `fav:<entry>`, `k:<credential>`, `s:<saved split>` (rd-core `SidebarTag`).

import { onLibraryChanged, type Api, type Listen, type Unlisten } from '../api';
import {
  EMPTY_LIBRARY,
  type ConnectionDto,
  type CredentialDto,
  type EntryDto,
  type FolderDto,
  type LibraryDto,
  type SavedSplitDto,
  type SearchResultDto,
} from '../types';
import { indexById, unique } from '../collections';
import { resolveDeps, type StoreDeps } from './deps';

export type TagKind = 'folder' | 'entry' | 'connection' | 'favorite' | 'credential' | 'split';

const PREFIXES: [string, TagKind][] = [
  ['f:', 'folder'],
  ['e:', 'entry'],
  ['c:', 'connection'],
  ['fav:', 'favorite'],
  ['k:', 'credential'],
  ['s:', 'split'],
];

/** `c:42` → `{kind: 'connection', id: '42'}`; `null` for an unknown prefix. */
export function parseTag(tag: string): { kind: TagKind; id: string } | null {
  for (const [prefix, kind] of PREFIXES) {
    if (tag.startsWith(prefix)) return { kind, id: tag.slice(prefix.length) };
  }
  return null;
}

/** The sidebar tag of an item. */
export function makeTag(kind: TagKind, id: string): string {
  const prefix = PREFIXES.find(([, k]) => k === kind)?.[0] ?? 'c:';
  return prefix + id;
}

/** Whether `tag` stands for an entry: an entry row (`e:`) or a favourite (`fav:`). */
export function isEntryKind(kind: TagKind | undefined): kind is 'entry' | 'favorite' {
  return kind === 'entry' || kind === 'favorite';
}

/**
 * The connection a row tag acts on: an entry's (`e:`, `fav:`) default connection, or the child row's
 * (`c:`) connection; `undefined` for other tags or a missing item.
 */
export function connectionOfTag(library: LibraryDto, tag: string): ConnectionDto | undefined {
  const parsed = parseTag(tag);
  if (!parsed) return undefined;
  if (parsed.kind === 'connection') return library.connections.find((c) => c.id === parsed.id);
  if (!isEntryKind(parsed.kind)) return undefined;
  const entry = library.entries.find((e) => e.id === parsed.id);
  const id = entry?.defaultConnectionId ?? entry?.connectionIds[0];
  return id ? library.connections.find((c) => c.id === id) : undefined;
}

/** What a session's tab is called: `<entry> · <label>` for an entry with 2+ connections, else the name. */
export function tabTitle(
  connection: Pick<ConnectionDto, 'name'> & Partial<Pick<ConnectionDto, 'displayLabel'>>,
  entry: Pick<EntryDto, 'connectionIds'> | undefined,
): string {
  const several = (entry?.connectionIds.length ?? 0) > 1;
  return several && connection.displayLabel
    ? `${connection.name} · ${connection.displayLabel}`
    : connection.name;
}

export interface LibraryStoreOptions extends StoreDeps {
  /** Search debounce in ms. */
  searchDelay?: number;
}

export class LibraryStore {
  /** The whole library; empty while locked. */
  library = $state<LibraryDto>(EMPTY_LIBRARY);
  /** Selected sidebar tags in selection order. */
  selection = $state<string[]>([]);
  /** The last plainly clicked or toggled tag: the start of a Shift-click range. */
  anchor = $state<string | null>(null);
  /** The sidebar search text. */
  query = $state('');
  /** Search hits; `null` when the query is empty (the sidebar shows the tree). */
  results = $state<SearchResultDto | null>(null);

  readonly entriesById = $derived(indexById(this.library.entries));
  readonly connectionsById = $derived(indexById(this.library.connections));
  readonly credentialsById = $derived(indexById(this.library.credentials));
  readonly foldersById = $derived(indexById(this.library.folders));
  readonly splitsById = $derived(indexById(this.library.savedSplits));

  /** The one selected tag, or `null` for none or several. */
  readonly single = $derived(this.selection.length === 1 ? (this.selection[0] ?? null) : null);
  /** The anchor when it is selected, else the first selected tag. */
  readonly primary = $derived(
    this.anchor && this.selection.includes(this.anchor) ? this.anchor : (this.selection[0] ?? null),
  );
  readonly isSearching = $derived(this.query.trim() !== '');

  #api: Api;
  #listen: Listen;
  #unlisten: Unlisten | undefined;
  #searchDelay: number;
  #searchTimer: ReturnType<typeof setTimeout> | undefined;
  #searchSeq = 0;

  constructor(options: LibraryStoreOptions = {}) {
    const { api, listen } = resolveDeps(options);
    this.#api = api;
    this.#listen = listen;
    this.#searchDelay = options.searchDelay ?? 150;
  }

  /** Subscribes to `library-changed` and loads the library. */
  async start() {
    this.#unlisten ??= await onLibraryChanged(this.#listen, (library) => this.replace(library));
    this.replace(await this.#api.library());
  }

  stop() {
    this.#unlisten?.();
    this.#unlisten = undefined;
    clearTimeout(this.#searchTimer);
  }

  /**
   * Takes a new library, drops selected tags whose item is gone and refreshes the search. A selected
   * connection (`c:`) of a one-connection entry becomes its entry row (`e:`): it has no row of its own.
   */
  replace(library: LibraryDto) {
    this.library = library;
    const kept = unique(this.selection.filter((tag) => this.exists(tag)).map((tag) => this.rowTag(tag)));
    if (kept.join('\n') !== this.selection.join('\n')) this.selection = kept;
    if (this.anchor) this.anchor = this.exists(this.anchor) ? this.rowTag(this.anchor) : null;
    if (this.isSearching) void this.runSearch();
  }

  /** The tag of the row showing `tag`: `e:<entry>` for a connection of a one-connection entry. */
  rowTag(tag: string): string {
    const parsed = parseTag(tag);
    if (parsed?.kind !== 'connection') return tag;
    const entry = this.entry(this.connection(parsed.id)?.entryId);
    return entry && entry.connectionIds.length === 1 ? `e:${entry.id}` : tag;
  }

  /** Selects a connection's row: its entry's when the entry has only it. */
  selectConnection(id: string) {
    this.select(this.rowTag(`c:${id}`));
  }

  // Lookups

  entry(id: string | null | undefined): EntryDto | undefined {
    return id ? this.entriesById.get(id) : undefined;
  }
  /** The entry's connections in order. */
  connectionsOf(entryId: string | null | undefined): ConnectionDto[] {
    return (this.entry(entryId)?.connectionIds ?? []).flatMap((id) => this.connection(id) ?? []);
  }
  /** The connection a row tag acts on (see `connectionOfTag`). */
  connectionForTag(tag: string | null | undefined): ConnectionDto | undefined {
    const parsed = tag ? parseTag(tag) : null;
    if (parsed?.kind === 'connection') return this.connection(parsed.id);
    if (!isEntryKind(parsed?.kind)) return undefined;
    const entry = this.entry(parsed.id);
    return this.connection(entry?.defaultConnectionId ?? entry?.connectionIds[0]);
  }

  connection(id: string | null | undefined): ConnectionDto | undefined {
    return id ? this.connectionsById.get(id) : undefined;
  }
  credential(id: string | null | undefined): CredentialDto | undefined {
    return id ? this.credentialsById.get(id) : undefined;
  }
  folder(id: string | null | undefined): FolderDto | undefined {
    return id ? this.foldersById.get(id) : undefined;
  }
  split(id: string | null | undefined): SavedSplitDto | undefined {
    return id ? this.splitsById.get(id) : undefined;
  }

  /** Whether the item behind `tag` exists. */
  exists(tag: string): boolean {
    const parsed = parseTag(tag);
    if (!parsed) return false;
    switch (parsed.kind) {
      case 'folder':
        return this.foldersById.has(parsed.id);
      case 'entry':
      case 'favorite':
        return this.entriesById.has(parsed.id);
      case 'connection':
        return this.connectionsById.has(parsed.id);
      case 'credential':
        return this.credentialsById.has(parsed.id);
      case 'split':
        return this.splitsById.has(parsed.id);
    }
  }

  // Selection

  isSelected(tag: string): boolean {
    return this.selection.includes(tag);
  }

  /** Plain click: only `tag`. */
  select(tag: string) {
    this.selection = [tag];
    this.anchor = tag;
  }

  /** ⌘/Ctrl-click: adds or removes `tag`. */
  toggle(tag: string) {
    this.selection = this.isSelected(tag)
      ? this.selection.filter((t) => t !== tag)
      : [...this.selection, tag];
    this.anchor = tag;
  }

  /** Shift-click: the range from the anchor to `tag` in the sidebar's visible `order`. */
  extendTo(tag: string, order: readonly string[]) {
    const from = this.anchor ? order.indexOf(this.anchor) : -1;
    const to = order.indexOf(tag);
    if (from < 0 || to < 0) {
      this.select(tag);
      return;
    }
    const [lo, hi] = from <= to ? [from, to] : [to, from];
    this.selection = order.slice(lo, hi + 1);
  }

  /** Replaces the selection (keeps the anchor when it stays selected). */
  setSelection(tags: readonly string[]) {
    this.selection = unique(tags);
    if (this.anchor && !this.selection.includes(this.anchor)) this.anchor = this.selection[0] ?? null;
  }

  /** Entry `entryId` collapsed: its selected child rows (`c:`) become the entry row (`e:`). */
  collapseEntrySelection(entryId: string) {
    const child = (tag: string | null) => {
      const parsed = tag ? parseTag(tag) : null;
      return parsed?.kind === 'connection' && this.connection(parsed.id)?.entryId === entryId;
    };
    if (this.selection.some(child)) {
      this.selection = unique(this.selection.map((tag) => (child(tag) ? `e:${entryId}` : tag)));
    }
    if (child(this.anchor)) this.anchor = `e:${entryId}`;
  }

  clearSelection() {
    this.selection = [];
    this.anchor = null;
  }

  /** The connections behind the selection (an entry's default, a child row's own), without duplicates. */
  get selectedConnections(): ConnectionDto[] {
    const connections = this.selection.flatMap((tag) => this.connectionForTag(tag) ?? []);
    return unique(connections.map((c) => c.id)).map((id) => this.connection(id)!);
  }

  // Search

  /** Sets the search text; the search runs after a short pause. An empty query clears the results. */
  setQuery(query: string) {
    this.query = query;
    clearTimeout(this.#searchTimer);
    if (!this.isSearching) {
      this.#searchSeq++;
      this.results = null;
      return;
    }
    this.#searchTimer = setTimeout(() => void this.runSearch(), this.#searchDelay);
  }

  /** Runs the search now; late answers to older queries are dropped. */
  async runSearch() {
    clearTimeout(this.#searchTimer);
    const seq = ++this.#searchSeq;
    const query = this.query;
    if (!query.trim()) {
      this.results = null;
      return;
    }
    try {
      const results = await this.#api.search(query);
      if (seq === this.#searchSeq) this.results = results;
    } catch {
      if (seq === this.#searchSeq) this.results = { entries: [], credentials: [] };
    }
  }
}

export function createLibraryStore(options: LibraryStoreOptions = {}) {
  return new LibraryStore(options);
}

export const library = createLibraryStore();
