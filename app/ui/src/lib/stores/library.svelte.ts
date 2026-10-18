// The library as Rust sends it (`library-changed` replaces it whole), the sidebar selection and the
// search. Selection entries are sidebar tags: `f:<folder>`, `c:<connection>`, `fav:<connection>`,
// `k:<credential>`, `s:<saved split>` (rd-core `SidebarTag`).

import { onLibraryChanged, type Api, type Listen, type Unlisten } from '../api';
import {
  EMPTY_LIBRARY,
  type ConnectionDto,
  type CredentialDto,
  type FolderDto,
  type LibraryDto,
  type SavedSplitDto,
  type SearchResultDto,
} from '../types';
import { indexById, unique } from '../collections';
import { resolveDeps, type StoreDeps } from './deps';

export type TagKind = 'folder' | 'connection' | 'favorite' | 'credential' | 'split';

const PREFIXES: [string, TagKind][] = [
  ['f:', 'folder'],
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

  /** Takes a new library, drops selected tags whose item is gone and refreshes the search. */
  replace(library: LibraryDto) {
    this.library = library;
    const kept = this.selection.filter((tag) => this.exists(tag));
    if (kept.length !== this.selection.length) this.selection = kept;
    if (this.anchor && !this.exists(this.anchor)) this.anchor = null;
    if (this.isSearching) void this.runSearch();
  }

  // Lookups

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
      case 'connection':
      case 'favorite':
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

  clearSelection() {
    this.selection = [];
    this.anchor = null;
  }

  /** The connections behind the selection (`c:` and `fav:`), without duplicates. */
  get selectedConnections(): ConnectionDto[] {
    const ids = unique(
      this.selection
        .map(parseTag)
        .filter((p) => p?.kind === 'connection' || p?.kind === 'favorite')
        .map((p) => p!.id),
    );
    return ids.map((id) => this.connection(id)).filter((c): c is ConnectionDto => c !== undefined);
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
      if (seq === this.#searchSeq) this.results = { connections: [], credentials: [] };
    }
  }
}

export function createLibraryStore(options: LibraryStoreOptions = {}) {
  return new LibraryStore(options);
}

export const library = createLibraryStore();
