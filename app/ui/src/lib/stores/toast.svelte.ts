// Bottom toasts (Swift `AppState.showToast`): one at a time, each auto-hides after ~2.5 s; messages
// shown meanwhile wait in a queue.

export interface Toast {
  id: number;
  message: string;
}

export interface ToastOptions {
  /** How long a toast stays, in ms. */
  duration?: number;
}

export function createToastStore({ duration = 2500 }: ToastOptions = {}) {
  let current = $state<Toast | null>(null);
  let queue = $state<Toast[]>([]);
  let nextId = 1;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function present(toast: Toast) {
    current = toast;
    timer = setTimeout(dismiss, duration);
  }

  /** Hides the current toast and shows the next queued one. */
  function dismiss() {
    clearTimeout(timer);
    timer = undefined;
    const [next, ...rest] = queue;
    queue = rest;
    current = null;
    if (next) present(next);
  }

  return {
    get current() {
      return current;
    },
    get pending() {
      return queue.length;
    },
    /** Shows `message` now, or after the toasts before it. Repeating the shown message is ignored. */
    show(message: string) {
      if (!message) return;
      if (current?.message === message && queue.length === 0) return;
      const toast = { id: nextId++, message };
      if (current) queue = [...queue, toast];
      else present(toast);
    },
    dismiss,
    /** Drops everything (tests, lock). */
    clear() {
      clearTimeout(timer);
      timer = undefined;
      queue = [];
      current = null;
    },
  };
}

export type ToastStore = ReturnType<typeof createToastStore>;

export const toast = createToastStore();
