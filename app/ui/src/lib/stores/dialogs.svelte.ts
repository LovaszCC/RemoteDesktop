// Confirmations and the one-field name prompt (Swift `confirmationDialog`s and `SavedSplitNameSheet`).
// One at a time; `DialogHost` renders the current one and the promise resolves with the answer.

export interface ConfirmRequest {
  title: string;
  /** May span several lines. */
  message?: string;
  confirmLabel: string;
  destructive?: boolean;
  /** A checkbox under the message (unchecked when it opens); see `confirmWithOption`. */
  option?: string;
}

export interface PromptRequest {
  title: string;
  /** Placeholder of the field. */
  label: string;
  initial?: string;
  confirmLabel: string;
}

export type Dialog =
  | ({ kind: 'confirm'; resolve: (ok: boolean, checked: boolean) => void } & ConfirmRequest)
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
    if (open?.kind === 'confirm') open.resolve(false, false);
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
    /** Like `confirm` with the request's checkbox: `{checked}` on confirm, `null` on cancel. */
    confirmWithOption(request: ConfirmRequest): Promise<{ checked: boolean } | null> {
      dismiss(close());
      return new Promise((resolve) => {
        current = { kind: 'confirm', ...request, resolve: (ok, checked) => resolve(ok ? { checked } : null) };
      });
    },
    /** Resolves the trimmed name, or `null` when cancelled or blank. */
    prompt(request: PromptRequest): Promise<string | null> {
      dismiss(close());
      return new Promise((resolve) => {
        current = { kind: 'prompt', ...request, resolve };
      });
    },
    /** The confirm button: a prompt's name, or a confirmation's checkbox. */
    accept(value?: string | boolean) {
      const open = close();
      if (open?.kind === 'confirm') open.resolve(true, value === true);
      else if (open?.kind === 'prompt') {
        const trimmed = (typeof value === 'string' ? value : '').trim();
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
