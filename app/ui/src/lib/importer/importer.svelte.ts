// The import flow (Swift `AppState.importRDM`, `chooseWinboxFile`, `loadWinbox`, `importWinbox`): pick a
// file in Rust's open panel, preview its secret-free rows (duplicates unchecked), then import the checked
// ones and toast the counts. Passwords never reach the webview: Rust parses the file again on commit.

import { api as defaultApi, type Api } from '../api';
import { errorMessage } from '../errors';
import { t } from '../i18n/index.svelte';
import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
import { allIncluded, initialIncluded, withIncluded } from './model';
import type { ImportKind, ImportPreviewDto, ImportResultDto, SkippedDto } from '../types';

export interface ImportDeps {
  api: Pick<Api, 'pickImportFile' | 'importPreview' | 'importCommit'>;
  toast: Pick<ToastStore, 'show'>;
  alerts: Pick<AlertStore, 'show'>;
}

/** The open preview: the file, its rows and which ones are checked. */
export interface ImportDraft {
  kind: ImportKind;
  path: string;
  preview: ImportPreviewDto;
  included: ReadonlySet<string>;
}

/** The sheet's title per importer. */
export const IMPORT_TITLES: Record<ImportKind, string> = {
  rdm: 'Import from RDM',
  winbox: 'Import from WinBox',
};

/** `portal (WebBrowser)`, `WebBrowser` without a name, or the localized `name (no host)`. */
export function skippedLabel(entry: SkippedDto): string {
  if (entry.reason === 'noHost') return t('%@ (no host)', entry.name);
  return entry.name ? `${entry.name} (${entry.kind})` : entry.kind;
}

/** The toast after an import (the Swift texts per importer). */
export function resultMessage(kind: ImportKind, result: ImportResultDto): string {
  if (kind === 'winbox') {
    return t(
      'Imported from WinBox — connections: %lld, new folders: %lld',
      result.connections,
      result.foldersCreated,
    );
  }
  return t(
    'Imported — connections: %lld, credentials: %lld, new folders: %lld.',
    result.connections,
    result.credentials,
    result.foldersCreated,
  );
}

export function createImportStore(deps: Partial<ImportDeps> = {}) {
  const api = deps.api ?? defaultApi;
  const toast = deps.toast ?? defaultToast;
  const alerts = deps.alerts ?? defaultAlerts;

  let draft = $state<ImportDraft | null>(null);
  let busy = $state(false);

  function fail(error: unknown) {
    alerts.show(errorMessage(error));
  }

  return {
    get draft() {
      return draft;
    },
    get busy() {
      return busy;
    },
    get includedCount() {
      return draft?.included.size ?? 0;
    },

    /** Picks a file and opens its preview; cancelling the panel does nothing. */
    async start(kind: ImportKind) {
      if (busy) return;
      busy = true;
      try {
        const path = await api.pickImportFile(kind);
        if (!path) return;
        const preview = await api.importPreview(kind, path);
        draft = { kind, path, preview, included: initialIncluded(preview.rows) };
      } catch (error) {
        fail(error);
      } finally {
        busy = false;
      }
    },

    isIncluded(id: string): boolean {
      return draft?.included.has(id) ?? false;
    },

    setIncluded(id: string, on: boolean) {
      if (!draft) return;
      draft = { ...draft, included: withIncluded(draft.included, id, on) };
    },

    /** Select all / Select none. */
    setAll(on: boolean) {
      if (!draft) return;
      draft = { ...draft, included: allIncluded(draft.preview.rows, on) };
    },

    close() {
      draft = null;
    },

    /** Imports the checked rows, closes the sheet and toasts the counts. */
    async commit() {
      const open = draft;
      if (!open || open.included.size === 0 || busy) return;
      busy = true;
      try {
        const selected = open.preview.rows.filter((r) => open.included.has(r.id)).map((r) => r.id);
        const result = await api.importCommit(open.kind, open.path, selected);
        draft = null;
        toast.show(resultMessage(open.kind, result));
      } catch (error) {
        fail(error);
      } finally {
        busy = false;
      }
    },
  };
}

export type ImportStore = ReturnType<typeof createImportStore>;

export const importer = createImportStore();
