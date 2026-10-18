// In-app updates (Swift `UpdateModel`): the silent launch check, "Check now" / "Check for Updates…", and
// Install → Restart. Rust does the work through the Tauri updater (`update_check`, `update_install`,
// `update_relaunch`); this store only keeps the phase the dialog and Settings show.

import { onUpdateProgress, type Unlisten } from '../api';
import { errorMessage } from '../errors';
import { resolveDeps, type StoreDeps } from '../stores/deps';
import type { UpdateCheckDto } from '../types';

/** A newer release found by the last check. */
export type AvailableUpdate = Extract<UpdateCheckDto, { kind: 'available' }>;

export type UpdatePhase =
  | { kind: 'idle' }
  | { kind: 'checking' }
  /** Development build without an updater key: nothing to check. */
  | { kind: 'notConfigured'; currentVersion: string }
  | { kind: 'upToDate'; currentVersion: string }
  | { kind: 'available'; update: AvailableUpdate }
  | { kind: 'installing'; update: AvailableUpdate; downloaded: number; total: number | null }
  /** Installed; a restart starts the new version (on Windows the installer has already taken over). */
  | { kind: 'installed'; update: AvailableUpdate }
  /** `update` is set when the install failed, so Try again retries the install instead of the check. */
  | { kind: 'failed'; message: string; update: AvailableUpdate | null };

export function createUpdateStore(deps: StoreDeps = {}) {
  const { api, listen } = resolveDeps(deps);
  let phase = $state<UpdatePhase>({ kind: 'idle' });
  let open = $state(false);

  const busy = () => phase.kind === 'checking' || phase.kind === 'installing';

  function apply(result: UpdateCheckDto, silent: boolean) {
    switch (result.kind) {
      case 'available':
        phase = { kind: 'available', update: result };
        if (silent) open = true;
        return;
      case 'upToDate':
      case 'notConfigured':
        if (!silent) phase = result;
        return;
      case 'skipped':
        return;
    }
  }

  return {
    get phase(): UpdatePhase {
      return phase;
    },
    /** Whether the update dialog is shown. */
    get isOpen() {
      return open;
    },

    /** "Check now" and the menu item: opens the dialog and always reports back. */
    async checkManually() {
      open = true;
      if (busy()) return;
      phase = { kind: 'checking' };
      try {
        apply(await api.updateCheck(false), false);
      } catch (error) {
        phase = { kind: 'failed', message: errorMessage(error), update: null };
      }
    },

    /** At launch: only when enabled and due (Rust decides); speaks up only when there is a newer version. */
    async checkAutomatically() {
      if (busy()) return;
      try {
        apply(await api.updateCheck(true), true);
      } catch {
        // Silent, like the Swift launch check.
      }
    },

    /** Downloads, verifies and installs the found update with progress. */
    async install() {
      const update =
        phase.kind === 'available' || (phase.kind === 'failed' && phase.update) ? phase.update : null;
      if (!update) return;
      phase = { kind: 'installing', update, downloaded: 0, total: null };
      let off: Unlisten | undefined;
      try {
        off = await onUpdateProgress(listen, ({ downloaded, total }) => {
          if (phase.kind === 'installing') phase = { ...phase, downloaded, total };
        });
        await api.updateInstall();
        phase = { kind: 'installed', update };
      } catch (error) {
        phase = { kind: 'failed', message: errorMessage(error), update };
      } finally {
        off?.();
      }
    },

    /** Restarts into the installed version. */
    async relaunch() {
      try {
        await api.updateRelaunch();
      } catch (error) {
        phase = { kind: 'failed', message: errorMessage(error), update: null };
      }
    },

    /** Closes the dialog; a running check or install goes on and Settings shows its state. */
    close() {
      open = false;
    },
  };
}

export type UpdateStore = ReturnType<typeof createUpdateStore>;

export const updates = createUpdateStore();
