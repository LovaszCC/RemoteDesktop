// The root view's state (Swift `AppState.phase`): which screen shows, the database path, read-only and the
// platform. Rust owns it; this store mirrors `app-status-changed` and wraps the session/database commands.

import { onAppStatusChanged, type Unlisten } from '../api';
import type { AppStatusDto, LockChoice, Phase, PickMode, Platform } from '../types';
import { resolveDeps, type StoreDeps } from './deps';

export function createAppStore(deps: StoreDeps = {}) {
  const { api, listen } = resolveDeps(deps);
  let status = $state<AppStatusDto | null>(null);
  let unlisten: Unlisten | undefined;

  /** Applies a status returned by a command (the event usually brings the same one). */
  function apply(next: AppStatusDto): AppStatusDto {
    status = next;
    return next;
  }

  return {
    get status() {
      return status;
    },
    /** `null` until the first status arrives. */
    get phase(): Phase | null {
      return status?.phase ?? null;
    },
    get platform(): Platform {
      return status?.platform ?? 'macos';
    },
    get readOnly() {
      return status?.readOnly ?? false;
    },
    get databasePath() {
      return status?.databasePath ?? null;
    },
    get isUnlocked() {
      return status?.phase.kind === 'unlocked';
    },

    /** Subscribes to `app-status-changed` and loads the current status. */
    async start() {
      unlisten ??= await onAppStatusChanged(listen, (next) => {
        status = next;
      });
      apply(await api.appStatus());
    },
    stop() {
      unlisten?.();
      unlisten = undefined;
    },

    // Session. Errors reject as `CommandError` (`wrongPassword`, …).
    createVault: async (password: string, remember: boolean) =>
      apply(await api.createVault(password, remember)),
    unlock: async (password: string, remember: boolean) => apply(await api.unlock(password, remember)),
    lock: async () => apply(await api.lock()),
    changePassword: (current: string, next: string) => api.changePassword(current, next),

    // Database
    openDatabase: async (path: string) => apply(await api.openDatabase(path)),
    createDatabase: async (path: string) => apply(await api.createDatabase(path)),
    /** "Retry" on the error screen. */
    reopenDatabase: async () => apply(await api.reopenDatabase()),
    resolveDbLock: async (choice: LockChoice) => apply(await api.resolveDbLock(choice)),
    revealDatabase: () => api.revealDatabase(),
    /**
     * "Open existing…" / "Create new…": shows the file dialog, then switches to the chosen file. `null`
     * when the dialog was cancelled.
     */
    async chooseDatabase(mode: PickMode): Promise<AppStatusDto | null> {
      const path = await api.pickDatabaseFile(mode);
      if (!path) return null;
      return apply(mode === 'open' ? await api.openDatabase(path) : await api.createDatabase(path));
    },
  };
}

export type AppStore = ReturnType<typeof createAppStore>;

export const app = createAppStore();
