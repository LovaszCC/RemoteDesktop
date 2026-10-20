// CSV import of a server list: the sheet explains the columns and offers the sample file, then previews
// the chosen file (entries, existing ones needing a choice, rows left out) and imports it. Rust parses the
// file again on commit.

import { api as defaultApi, type Api } from '../api';
import { errorMessage } from '../errors';
import { t } from '../i18n/index.svelte';
import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
import type { CsvPreviewDto, CsvSkippedDto } from '../types';
import { decisions, initialImportIncluded, unanswered, withAll, type ConflictChoice } from './model';
import { transferMessage } from './transferImporter.svelte';

export interface CsvImportDeps {
  api: Pick<Api, 'saveCsvSample' | 'pickCsvFile' | 'csvPreview' | 'csvCommit'>;
  toast: Pick<ToastStore, 'show'>;
  alerts: Pick<AlertStore, 'show'>;
}

/** The chosen file's preview. */
export interface CsvDraft {
  path: string;
  preview: CsvPreviewDto;
  included: ReadonlySet<string>;
  choices: Readonly<Record<string, ConflictChoice>>;
}

/** `Line 4: box (unknown type: vnc)`. */
export function csvSkippedLabel(row: CsvSkippedDto): string {
  let why: string;
  switch (row.reason) {
    case 'noName':
      why = t('no name');
      break;
    case 'noHost':
      why = t('no host');
      break;
    case 'unknownType':
      why = t('unknown type: %@', row.detail);
      break;
    case 'badUrl':
      why = t('invalid URL: %@', row.detail);
      break;
  }
  return t('Line %lld: %@', row.line, row.name ? `${row.name} (${why})` : why);
}

export function createCsvImportStore(deps: Partial<CsvImportDeps> = {}) {
  const api = deps.api ?? defaultApi;
  const toast = deps.toast ?? defaultToast;
  const alerts = deps.alerts ?? defaultAlerts;

  let open = $state(false);
  let draft = $state<CsvDraft | null>(null);
  let busy = $state(false);

  async function run(work: () => Promise<void>) {
    if (busy) return;
    busy = true;
    try {
      await work();
    } catch (error) {
      alerts.show(errorMessage(error));
    } finally {
      busy = false;
    }
  }

  return {
    get open() {
      return open;
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

    /** Opens the sheet on its explanation. */
    start() {
      open = true;
      draft = null;
    },

    /** Saves the sample file where the user picks. */
    saveSample() {
      return run(async () => {
        if (await api.saveCsvSample()) toast.show(t('Sample saved'));
      });
    },

    /** Picks a CSV file and previews it; cancelling the panel keeps the current state. */
    choose() {
      return run(async () => {
        const path = await api.pickCsvFile();
        if (!path) return;
        const preview = await api.csvPreview(path);
        draft = { path, preview, included: initialImportIncluded(preview.rows), choices: {} };
      });
    },

    setIncluded(ids: readonly string[], on: boolean) {
      if (draft) draft = { ...draft, included: withAll(draft.included, ids, on) };
    },

    setChoice(id: string, choice: ConflictChoice) {
      if (draft) draft = { ...draft, choices: { ...draft.choices, [id]: choice } };
    },

    close() {
      open = false;
      draft = null;
    },

    /** Imports the checked rows as chosen, closes the sheet and toasts the counts. */
    commit() {
      const current = draft;
      if (!current || current.included.size === 0 || this.unanswered > 0) return Promise.resolve();
      return run(async () => {
        const result = await api.csvCommit(
          current.path,
          decisions(current.preview.rows, current.included, current.choices),
        );
        open = false;
        draft = null;
        toast.show(transferMessage(result));
      });
    },
  };
}

export type CsvImportStore = ReturnType<typeof createCsvImportStore>;

export const csvImporter = createCsvImportStore();
