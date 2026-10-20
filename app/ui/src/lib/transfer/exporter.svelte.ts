// The export flow: pick entries and credentials (starting with the sidebar selection), optionally a
// password, then Rust's save panel and the file. Passwords of the records never reach the webview; Rust
// reads them from the vault only for an encrypted export.

import { api as defaultApi, type Api } from '../api';
import { errorMessage } from '../errors';
import { t } from '../i18n/index.svelte';
import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
import type { ExportResultDto, LibraryDto } from '../types';
import { exportProblem, initialSelection, withAll } from './model';

export interface ExportDeps {
  api: Pick<Api, 'pickExportPath' | 'exportLibrary'>;
  toast: Pick<ToastStore, 'show'>;
  alerts: Pick<AlertStore, 'show'>;
}

/** The open sheet. */
export interface ExportDraft {
  entries: ReadonlySet<string>;
  credentials: ReadonlySet<string>;
  password: string;
  confirm: string;
}

/** The toast after an export. */
export function exportMessage(result: ExportResultDto): string {
  return result.encrypted
    ? t('Exported with password — entries: %lld, credentials: %lld', result.entries, result.credentials)
    : t('Exported without passwords — entries: %lld, credentials: %lld', result.entries, result.credentials);
}

export function createExportStore(deps: Partial<ExportDeps> = {}) {
  const api = deps.api ?? defaultApi;
  const toast = deps.toast ?? defaultToast;
  const alerts = deps.alerts ?? defaultAlerts;

  let draft = $state<ExportDraft | null>(null);
  let busy = $state(false);

  return {
    get draft() {
      return draft;
    },
    get busy() {
      return busy;
    },
    /** `nothing`, `mismatch` or `null` when the export can start. */
    get problem() {
      if (!draft) return 'nothing';
      return exportProblem(draft.entries.size, draft.credentials.size, draft.password, draft.confirm);
    },

    /** Opens the sheet with the records behind the sidebar selection checked. */
    open(library: LibraryDto, selection: readonly string[]) {
      const initial = initialSelection(library, selection);
      draft = { ...initial, password: '', confirm: '' };
    },

    close() {
      draft = null;
    },

    setEntries(ids: readonly string[], on: boolean) {
      if (draft) draft = { ...draft, entries: withAll(draft.entries, ids, on) };
    },

    setCredentials(ids: readonly string[], on: boolean) {
      if (draft) draft = { ...draft, credentials: withAll(draft.credentials, ids, on) };
    },

    setPassword(password: string) {
      if (draft) draft = { ...draft, password };
    },

    setConfirm(confirm: string) {
      if (draft) draft = { ...draft, confirm };
    },

    /** Asks where to save, writes the file, closes the sheet and toasts the counts. */
    async commit() {
      const open = draft;
      if (!open || busy || this.problem) return;
      busy = true;
      try {
        const path = await api.pickExportPath();
        if (!path) return;
        const result = await api.exportLibrary(
          path,
          [...open.entries],
          [...open.credentials],
          open.password === '' ? null : open.password,
        );
        draft = null;
        toast.show(exportMessage(result));
      } catch (error) {
        alerts.show(errorMessage(error));
      } finally {
        busy = false;
      }
    },
  };
}

export type ExportStore = ReturnType<typeof createExportStore>;

export const exporter = createExportStore();
