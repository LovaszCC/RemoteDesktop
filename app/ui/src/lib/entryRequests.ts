// Entries opened from the system search (Spotlight, the Start menu, the desktop search): the launcher
// starts or focuses the app, Rust keeps the entry id until the library is unlocked, and this opens it
// like a sidebar double-click. Checked on `entry-requested`, on every `library-changed` (the unlock) and
// once the library has loaded.

import { onEntryRequested, onLibraryChanged, type Unlisten } from './api';
import { t } from './i18n/index.svelte';
import { sidebarActions } from './sidebar/actions.svelte';
import { resolveDeps, type StoreDeps } from './stores/deps';
import { library as defaultLibrary, type LibraryStore } from './stores/library.svelte';
import { toast as defaultToast, type ToastStore } from './stores/toast.svelte';

export interface EntryRequestDeps extends StoreDeps {
  library?: Pick<LibraryStore, 'entry' | 'replace'>;
  /** Runs a sidebar row's double-click action. */
  open?: (tag: string) => void;
  toast?: ToastStore;
}

/** Starts listening; `check` takes and opens a waiting request, `stop` unsubscribes. */
export function installEntryRequests(deps: EntryRequestDeps = {}) {
  const { api, listen } = resolveDeps(deps);
  const library = deps.library ?? defaultLibrary;
  const open = deps.open ?? ((tag: string) => sidebarActions.primaryAction(tag));
  const toast = deps.toast ?? defaultToast;
  const unlisteners: Unlisten[] = [];
  let stopped = false;

  async function check() {
    const id = await api.takeEntryRequest().catch(() => null);
    if (!id) return;
    // The store may not have the new library yet (this can run before its own listener).
    if (!library.entry(id)) library.replace(await api.library());
    if (library.entry(id)) open(`e:${id}`);
    else toast.show(t('Entry not found'));
  }

  for (const subscribe of [onEntryRequested, onLibraryChanged]) {
    void subscribe(listen, () => void check())
      .then((off) => {
        if (stopped) off();
        else unlisteners.push(off);
      })
      .catch(() => {});
  }

  return {
    check,
    stop() {
      stopped = true;
      unlisteners.splice(0).forEach((off) => off());
    },
  };
}
