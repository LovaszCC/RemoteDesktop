// The error alert (Swift: an alert titled "RemoteDesktop" with OK). One at a time; later ones wait.

export function createAlertStore() {
  let queue = $state<string[]>([]);

  return {
    /** The message on screen, or `null`. */
    get current(): string | null {
      return queue[0] ?? null;
    },
    show(message: string) {
      if (message) queue = [...queue, message];
    },
    /** OK: closes the current alert. */
    dismiss() {
      queue = queue.slice(1);
    },
  };
}

export type AlertStore = ReturnType<typeof createAlertStore>;

export const alerts = createAlertStore();
