// Confirmations and the one-field name prompt (Swift `confirmationDialog`s and `SavedSplitNameSheet`).
// One at a time; `DialogHost` renders the current one and the promise resolves with the answer.

export interface ConfirmRequest {
  title: string;
  /** May span several lines. */
  message?: string;
  confirmLabel: string;
  destructive?: boolean;
}

export interface PromptRequest {
  title: string;
  /** Placeholder of the field. */
  label: string;
  initial?: string;
  confirmLabel: string;
}

export type Dialog =
  | ({ kind: 'confirm'; resolve: (ok: boolean) => void } & ConfirmRequest)
  | ({ kind: 'prompt'; resolve: (name: string | null) => void } & PromptRequest);

export function createDialogStore() {
  let current = $state<Dialog | null>(null);

  function close() {
    const open = current;
    current = null;
    return open;
  }

  /** Answers a dialog that was not accepted. */
  function dismiss(open: Dialog | null) {
    if (open?.kind === 'confirm') open.resolve(false);
    else open?.resolve(null);
  }

  return {
    get current() {
      return current;
    },
    /** Resolves `true` on confirm, `false` on cancel (or when another dialog replaces it). */
    confirm(request: ConfirmRequest): Promise<boolean> {
      dismiss(close());
      return new Promise((resolve) => {
        current = { kind: 'confirm', ...request, resolve };
      });
    },
    /** Resolves the trimmed name, or `null` when cancelled or blank. */
    prompt(request: PromptRequest): Promise<string | null> {
      dismiss(close());
      return new Promise((resolve) => {
        current = { kind: 'prompt', ...request, resolve };
      });
    },
    /** The confirm button. */
    accept(name?: string) {
      const open = close();
      if (open?.kind === 'confirm') open.resolve(true);
      else if (open?.kind === 'prompt') {
        const trimmed = (name ?? '').trim();
        open.resolve(trimmed === '' ? null : trimmed);
      }
    },
    /** Cancel, Esc. */
    cancel() {
      dismiss(close());
    },
  };
}

export type DialogStore = ReturnType<typeof createDialogStore>;

export const dialogs = createDialogStore();
