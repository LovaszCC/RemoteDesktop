// The state of one Files window (Swift `FileBrowserSession` + `LocalBrowser` + `TransferQueue`): the
// connection, both panes' listings and selections, the hidden-files and gzip toggles, the running
// transfer's progress, the conflict question and the last result. Events are filtered by browser id.

import {
  onFilesConflict,
  onFilesDone,
  onFilesHostKeyPrompt,
  onFilesProgress,
  onFilesStatus,
  type Unlisten,
} from '../api';
import { errorMessage } from '../errors';
import { createHostKeyStore } from '../forwards/hostKeys.svelte';
import { resolveDeps, type StoreDeps } from '../stores/deps';
import type {
  ConflictChoice,
  FileEntryDto,
  FilesConflictDto,
  FilesDoneDto,
  FilesProgressDto,
  FilesStatusDto,
  ListingDto,
  TransferDirection,
} from '../types';
import { groupByFolder, joinLocal, joinRemote, sortEntries, type SortOrder } from './model';

export type Side = 'remote' | 'local';

export interface Pane {
  listing: ListingDto | null;
  error: string | null;
  loading: boolean;
  selection: string[];
  anchor: string | null;
  sort: SortOrder;
}

function emptyPane(): Pane {
  return {
    listing: null,
    error: null,
    loading: false,
    selection: [],
    anchor: null,
    sort: { key: 'name', ascending: true },
  };
}

export function createFilesStore(browserId: string, deps: StoreDeps = {}) {
  const { api, listen } = resolveDeps(deps);
  let status = $state<FilesStatusDto | null>(null);
  let statusError = $state<string | null>(null);
  const panes = $state<Record<Side, Pane>>({ remote: emptyPane(), local: emptyPane() });
  let showHidden = $state(false);
  let gzip = $state(true);
  let progress = $state<FilesProgressDto | null>(null);
  let transferId = $state<string | null>(null);
  let conflict = $state<FilesConflictDto | null>(null);
  let lastDone = $state<FilesDoneDto | null>(null);
  let actionError = $state<string | null>(null);
  let unlisten: Unlisten[] = [];
  let started = false;

  const hostKeys = createHostKeyStore(deps, {
    prompts: (l, add) =>
      onFilesHostKeyPrompt(l, (prompt) => {
        if (prompt.browserId === browserId) add(prompt);
      }),
  });

  const mine = <T extends { browserId: string }>(handler: (payload: T) => void) => {
    return (payload: T) => {
      if (payload.browserId === browserId) handler(payload);
    };
  };

  async function list(side: Side, path: string | null) {
    const pane = panes[side];
    pane.loading = true;
    try {
      const listing =
        side === 'remote'
          ? await api.filesListRemote(browserId, path ?? '~', showHidden)
          : await api.filesListLocal(path, showHidden);
      const changed = pane.listing?.path !== listing.path;
      pane.listing = listing;
      pane.error = null;
      const names = listing.entries.map((e) => e.name);
      pane.selection = changed ? [] : pane.selection.filter((n) => names.includes(n));
      if (changed) pane.anchor = null;
    } catch (error) {
      pane.error = errorMessage(error);
    } finally {
      pane.loading = false;
    }
  }

  function applyStatus(next: FilesStatusDto) {
    const wasReady = status?.state === 'ready';
    status = next;
    if (next.state === 'ready' && !wasReady) {
      void list('remote', panes.remote.listing?.path ?? next.startPath);
    }
  }

  function sources(side: Side, names: readonly string[]): string[] {
    const path = panes[side].listing?.path ?? '';
    return names.map((name) => (side === 'remote' ? joinRemote(path, name) : joinLocal(path, name)));
  }

  async function start(direction: TransferDirection, paths: string[], destination: string) {
    if (paths.length === 0 || transferId !== null) return;
    actionError = null;
    lastDone = null;
    try {
      transferId = await api.filesTransfer(browserId, direction, paths, destination, gzip);
    } catch (error) {
      actionError = errorMessage(error);
    }
  }

  const store = {
    browserId,
    hostKeys,
    get status() {
      return status;
    },
    /** `files_info` failed (the browser is gone). */
    get statusError() {
      return statusError;
    },
    get ready() {
      return status?.state === 'ready';
    },
    get remote(): Pane {
      return panes.remote;
    },
    get local(): Pane {
      return panes.local;
    },
    pane(side: Side): Pane {
      return panes[side];
    },
    /** The pane's rows: sorted, folders first. */
    rows(side: Side): FileEntryDto[] {
      return sortEntries(panes[side].listing?.entries ?? [], panes[side].sort);
    },
    get showHidden() {
      return showHidden;
    },
    get gzip() {
      return gzip;
    },
    set gzip(value: boolean) {
      gzip = value;
    },
    get progress() {
      return progress;
    },
    get transferring() {
      return transferId !== null;
    },
    get conflict() {
      return conflict;
    },
    get lastDone() {
      return lastDone;
    },
    get actionError() {
      return actionError;
    },
    dismissResult() {
      lastDone = null;
      actionError = null;
    },

    async start() {
      if (started) return;
      started = true;
      unlisten = [
        await onFilesStatus(listen, mine(applyStatus)),
        await onFilesProgress(
          listen,
          mine((p) => {
            if (p.transferId === transferId || transferId === null) {
              transferId = p.transferId;
              progress = p;
            }
          }),
        ),
        await onFilesConflict(
          listen,
          mine((c) => {
            conflict = c;
          }),
        ),
        await onFilesDone(
          listen,
          mine((done) => {
            transferId = null;
            progress = null;
            conflict = null;
            lastDone = done;
            // The destination pane shows the new items.
            void store.list(done.direction === 'download' ? 'local' : 'remote', null);
          }),
        ),
      ];
      await hostKeys.start();
      void list('local', null);
      try {
        applyStatus(await api.filesInfo(browserId));
      } catch (error) {
        statusError = errorMessage(error);
      }
    },
    stop() {
      for (const off of unlisten) off();
      unlisten = [];
      hostKeys.stop();
      started = false;
    },

    /** Lists a folder; `null` re-lists the current one (or the default local folder). */
    list(side: Side, path: string | null) {
      const current = panes[side].listing?.path ?? null;
      return list(side, path ?? current);
    },
    refresh() {
      return Promise.all([store.list('remote', null), store.list('local', null)]);
    },
    up(side: Side) {
      const parent = panes[side].listing?.parent;
      return parent ? list(side, parent) : Promise.resolve();
    },
    /** Double click: folders (and remote links) open. */
    open(side: Side, entry: FileEntryDto) {
      const openable = entry.kind === 'directory' || (side === 'remote' && entry.kind === 'symlink');
      if (!openable) return Promise.resolve();
      return list(side, sources(side, [entry.name])[0] ?? null);
    },
    async setShowHidden(value: boolean) {
      showHidden = value;
      await store.refresh();
    },
    setSelection(side: Side, selection: string[], anchor: string | null) {
      panes[side].selection = selection;
      panes[side].anchor = anchor;
    },
    setSort(side: Side, sort: SortOrder) {
      panes[side].sort = sort;
    },

    /** Copies `names` of `from` into the other pane's folder. */
    copy(from: Side, names: readonly string[]) {
      const to: Side = from === 'remote' ? 'local' : 'remote';
      const destination = panes[to].listing?.path;
      if (!destination || !store.ready) return Promise.resolve();
      return start(from === 'remote' ? 'download' : 'upload', sources(from, names), destination);
    },
    /** Uploads local paths from anywhere (an OS drop) into the remote folder, one transfer per call. */
    uploadPaths(paths: readonly string[]) {
      const destination = panes.remote.listing?.path;
      if (!destination || !store.ready || paths.length === 0) return Promise.resolve();
      // One transfer at a time: the folders' items go together, each with its full path.
      const ordered = groupByFolder(paths).flatMap((g) => g.paths);
      return start('upload', ordered, destination);
    },
    cancel() {
      conflict = null;
      return api.filesCancel(browserId).catch(() => {});
    },
    answerConflict(choice: ConflictChoice, applyToAll: boolean) {
      const question = conflict;
      if (!question) return Promise.resolve();
      conflict = null;
      return api.filesConflictAnswer(question.requestId, choice, applyToAll).catch(() => {});
    },
    async reconnect() {
      try {
        await api.filesReconnect(browserId);
      } catch (error) {
        statusError = errorMessage(error);
      }
    },
  };
  return store;
}

export type FilesStore = ReturnType<typeof createFilesStore>;
