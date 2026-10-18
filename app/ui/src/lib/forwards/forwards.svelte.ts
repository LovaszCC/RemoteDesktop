// Port forward tunnels (Swift `AppState.portForwardSessions`): the state of every started forward from
// `port-forward-status`, and the start/stop/save/delete commands. Rust is the source of truth; a
// `stopped` status drops the forward from the map, a failed one stays to show its error.

import { onPortForwardStatus, type Unlisten } from '../api';
import { resolveDeps, type StoreDeps } from '../stores/deps';
import type { ForwardStatusDto, PortForwardDraft } from '../types';

export function isRunning(status: ForwardStatusDto | null | undefined): boolean {
  return status?.state === 'starting' || status?.state === 'active';
}

export function createForwardStore(deps: StoreDeps = {}) {
  const { api, listen } = resolveDeps(deps);
  let statuses = $state<Record<string, ForwardStatusDto>>({});
  let unlisten: Unlisten | null = null;

  function apply(status: ForwardStatusDto) {
    if (status.state === 'stopped') {
      const rest = { ...statuses };
      delete rest[status.id];
      statuses = rest;
    } else {
      statuses = { ...statuses, [status.id]: status };
    }
  }

  return {
    /** The forward's state, or `null` when it was never started or is stopped. */
    status(id: string): ForwardStatusDto | null {
      return statuses[id] ?? null;
    },
    /** Whether a forward of the connection is starting or active (the sidebar's green dot). */
    hasRunning(connectionId: string): boolean {
      return Object.values(statuses).some((s) => s.connectionId === connectionId && isRunning(s));
    },
    async start() {
      if (unlisten) return;
      unlisten = await onPortForwardStatus(listen, apply);
      const current = await api.portForwardStatuses();
      statuses = Object.fromEntries(current.map((s) => [s.id, s]));
    },
    stop() {
      unlisten?.();
      unlisten = null;
    },
    /** Exposed for tests. */
    apply,
    /** Saves the editor's draft and optionally starts it; rejects with the validation error. */
    async save(draft: PortForwardDraft, start: boolean): Promise<string> {
      const id = await api.portForwardSave(draft);
      if (start) await api.portForwardStart(id);
      return id;
    },
    startForward: (id: string) => api.portForwardStart(id),
    stopForward: (id: string) => api.portForwardStop(id),
    remove: (id: string) => api.portForwardDelete(id),
  };
}

export type ForwardStore = ReturnType<typeof createForwardStore>;

export const forwards = createForwardStore();
