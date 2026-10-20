// Import of the app's own export: pick the file, ask its password when it is encrypted, preview its
// secret-free rows, let the user choose what happens with records that already exist, then import. Rust
// reads the file again on commit; the export's password is the only secret that passes through here.

import { api as defaultApi, toCommandError, type Api } from '../api';
import { errorMessage } from '../errors';
import { t } from '../i18n/index.svelte';
import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
import type { TransferPreviewDto, TransferResultDto } from '../types';
import { decisions, initialImportIncluded, unanswered, withAll, type ConflictChoice } from './model';

export interface TransferImportDeps {
  api: Pick<Api, 'pickTransferFile' | 'transferPreview' | 'transferCommit'>;
  toast: Pick<ToastStore, 'show'>;
  alerts: Pick<AlertStore, 'show'>;
}

/** Waiting for the password of an encrypted file. */
export interface PasswordStep {
  path: string;
  fileName: string;
  /** The last try was wrong. */
  wrong: boolean;
}

/** The open preview. */
export interface TransferDraft {
  path: string;
  password: string | null;
  preview: TransferPreviewDto;
  included: ReadonlySet<string>;
  choices: Readonly<Record<string, ConflictChoice>>;
}

/** The toast after an import. */
export function transferMessage(result: TransferResultDto): string {
  return t(
    'Imported — new entries: %lld, replaced: %lld, new credentials: %lld, replaced: %lld',
    result.entriesCreated,
    result.entriesUpdated,
    result.credentialsCreated,
    result.credentialsUpdated,
  );
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function createTransferImportStore(deps: Partial<TransferImportDeps> = {}) {
  const api = deps.api ?? defaultApi;
  const toast = deps.toast ?? defaultToast;
  const alerts = deps.alerts ?? defaultAlerts;

  let passwordStep = $state<PasswordStep | null>(null);
  let draft = $state<TransferDraft | null>(null);
  let busy = $state(false);

  /** Reads the file; asks for the password when it is encrypted. */
  async function load(path: string, password: string | null) {
    try {
      const preview = await api.transferPreview(path, password);
      passwordStep = null;
      draft = { path, password, preview, included: initialImportIncluded(preview.rows), choices: {} };
    } catch (error) {
      const code = toCommandError(error).code;
      if (code === 'transferNeedsPassword' || code === 'transferWrongPassword') {
        passwordStep = { path, fileName: fileName(path), wrong: code === 'transferWrongPassword' };
      } else {
        passwordStep = null;
        alerts.show(errorMessage(error));
      }
    }
  }

  return {
    get passwordStep() {
      return passwordStep;
    },
    get draft() {
      return draft;
    },
    get busy() {
      return busy;
    },
    get includedCount() {
      return draft?.included.size ?? 0;
    },
    /** Checked conflict rows without a choice yet. */
    get unanswered() {
      return draft ? unanswered(draft.preview.rows, draft.included, draft.choices) : 0;
    },

    /** Picks a file and opens its preview (or the password step); cancelling the panel does nothing. */
    async start() {
      if (busy) return;
      busy = true;
      try {
        const path = await api.pickTransferFile();
        if (path) await load(path, null);
      } catch (error) {
        alerts.show(errorMessage(error));
      } finally {
        busy = false;
      }
    },

    /** Tries the password of the file in the password step. */
    async unlock(password: string) {
      const step = passwordStep;
      if (!step || busy || password === '') return;
      busy = true;
      try {
        await load(step.path, password);
      } finally {
        busy = false;
      }
    },

    setIncluded(ids: readonly string[], on: boolean) {
      if (draft) draft = { ...draft, included: withAll(draft.included, ids, on) };
    },

    setChoice(id: string, choice: ConflictChoice) {
      if (draft) draft = { ...draft, choices: { ...draft.choices, [id]: choice } };
    },

    close() {
      passwordStep = null;
      draft = null;
    },

    /** Imports the checked rows as chosen, closes the sheet and toasts the counts. */
    async commit() {
      const open = draft;
      if (!open || open.included.size === 0 || busy || this.unanswered > 0) return;
      busy = true;
      try {
        const result = await api.transferCommit(
          open.path,
          open.password,
          decisions(open.preview.rows, open.included, open.choices),
        );
        draft = null;
        toast.show(transferMessage(result));
      } catch (error) {
        alerts.show(errorMessage(error));
      } finally {
        busy = false;
      }
    },
  };
}

export type TransferImportStore = ReturnType<typeof createTransferImportStore>;

export const transferImporter = createTransferImportStore();
