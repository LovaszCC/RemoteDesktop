// The embedded RDP sessions (Swift `AppState` RDP lifecycle): tab id = Rust session id → state, plus the
// session's backing canvas. Opening adds a tab; closing the tab (any way) ends the session. Frames arrive
// on the session's channel, are painted at once and acknowledged (Rust keeps at most 2 in flight);
// `rdp-event`s update the state.

import { Channel } from '@tauri-apps/api/core';
import { createApi, onRdpEvent, tauriBridge, type Api, type Listen, type Unlisten } from '../api';
import { errorMessage } from '../errors';
import { t } from '../i18n/index.svelte';
import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
import { dialogs as defaultDialogs, type DialogStore } from '../stores/dialogs.svelte';
import { library as defaultLibrary, tabTitle, type LibraryStore } from '../stores/library.svelte';
import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
import { tabs as defaultTabs, type TabsStore } from '../tabs.svelte';
import type { ConnectionDto, RdpEventDto, RdpInput } from '../types';
import { decodeFrame, frameBytes } from './frame';
import {
  applyRdpEvent,
  disconnected,
  initialViewSize,
  newRdpState,
  type RdpSessionState,
  type RdpSize,
} from './model';
import { createCanvasSurface, type RdpSurface, type SurfaceFactory } from './surface';

/** What receives a session's frames (a Tauri `Channel`); tests pass plain objects. */
export interface FrameChannel {
  onmessage: (data: unknown) => void;
}

export interface RdpSessionsDeps {
  api: Api;
  listen: Listen;
  tabs: TabsStore;
  toast: Pick<ToastStore, 'show'>;
  alerts: Pick<AlertStore, 'show'>;
  dialogs: Pick<DialogStore, 'confirm'>;
  createSurface: SurfaceFactory;
  createChannel: () => FrameChannel;
  /** The size of a view not laid out yet. */
  defaultSize: () => RdpSize;
  /** The entries, for tab titles and groups. */
  library: Pick<LibraryStore, 'entry'>;
}

/** What opening needs of a connection; `entryId`/`displayLabel` name and group the tab per entry. */
export type SessionConnection = Pick<ConnectionDto, 'id' | 'name' | 'type' | 'color'> &
  Partial<Pick<ConnectionDto, 'entryId' | 'displayLabel'>>;

/** Events kept for a session whose `open_rdp_session` hasn't answered yet. */
const MAX_PENDING = 64;

export function createRdpSessionsStore(deps: Partial<RdpSessionsDeps> = {}) {
  const api = deps.api ?? createApi(tauriBridge.invoke);
  const listen = deps.listen ?? tauriBridge.listen;
  const tabs = deps.tabs ?? defaultTabs;
  const toast = deps.toast ?? defaultToast;
  const alerts = deps.alerts ?? defaultAlerts;
  const dialogs = deps.dialogs ?? defaultDialogs;
  const createSurface = deps.createSurface ?? createCanvasSurface;
  const createChannel = deps.createChannel ?? (() => new Channel<ArrayBuffer>() as unknown as FrameChannel);
  const defaultSize = deps.defaultSize ?? (() => initialViewSize(window));
  const library = deps.library ?? defaultLibrary;

  let states = $state<Record<string, RdpSessionState>>({});
  // Not state: surfaces, sizes and queued events don't render (`states` does).
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const surfaces = new Map<string, RdpSurface>();
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const viewSizes = new Map<string, RdpSize>();
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const pending = new Map<string, RdpEventDto[]>();
  let lastSize: RdpSize | null = null;
  let unlisten: Unlisten | null = null;

  function set(state: RdpSessionState) {
    states = { ...states, [state.id]: state };
  }

  const ack = (id: string) => void api.rdpAckFrame(id).catch(() => {});

  /** Ends a session whose tab closed. */
  function forget(id: string) {
    if (!(id in states)) return;
    const rest = { ...states };
    delete rest[id];
    states = rest;
    pending.delete(id);
    viewSizes.delete(id);
    surfaces.get(id)?.dispose();
    surfaces.delete(id);
    void api.rdpClose(id).catch(() => {});
  }

  const offClosed = tabs.onSessionClosed(forget);

  function handleEvent(event: RdpEventDto) {
    const state = states[event.sessionId];
    if (!state) {
      const queue = pending.get(event.sessionId) ?? [];
      if (queue.length < MAX_PENDING) pending.set(event.sessionId, [...queue, event]);
      return;
    }
    const { state: next, effect } = applyRdpEvent(state, event);
    set(next);
    if (effect?.kind === 'close') tabs.closeSession(next.id);
    else if (effect?.kind === 'toast') toast.show(effect.message);
  }

  const sizeOf = (id: string): RdpSize => viewSizes.get(id) ?? lastSize ?? defaultSize();

  const store = {
    get states(): Readonly<Record<string, RdpSessionState>> {
      return states;
    },
    state(id: string | null): RdpSessionState | undefined {
      return id === null ? undefined : states[id];
    },
    has(id: string): boolean {
      return id in states;
    },
    surface(id: string): RdpSurface | undefined {
      return surfaces.get(id);
    },
    /** The active tab's session, if it is an RDP tab. */
    get active(): RdpSessionState | undefined {
      return store.state(tabs.active);
    },

    /** Subscribes to `rdp-event`. */
    async start() {
      if (!unlisten) unlisten = await onRdpEvent(listen, handleEvent);
    },
    stop() {
      unlisten?.();
      unlisten = null;
    },
    /** Stops listening to tab closes too (tests). */
    dispose() {
      store.stop();
      offClosed();
    },
    handleEvent,

    /** Opens an embedded session for `connection` in a new tab; rejects with the `CommandError`. */
    async open(connection: SessionConnection): Promise<string> {
      const surface = createSurface();
      const channel = createChannel();
      let sessionId: string | null = null;
      let unacked = 0;
      channel.onmessage = (data) => {
        const bytes = frameBytes(data);
        const frame = bytes ? decodeFrame(bytes) : null;
        if (frame) surface.paint(frame);
        if (sessionId) ack(sessionId);
        else unacked += 1;
      };
      const size = lastSize ?? defaultSize();
      try {
        ({ sessionId } = await api.openRdpSession(connection.id, size.width, size.height, channel));
      } catch (error) {
        surface.dispose();
        throw error;
      }
      const id = sessionId;
      surfaces.set(id, surface);
      const title = tabTitle(connection, library.entry(connection.entryId));
      states = { ...states, [id]: newRdpState(id, connection.id, title) };
      tabs.openSession({
        id,
        title,
        type: connection.type,
        color: connection.color,
        // An entry's tabs sit together and are numbered.
        group: connection.entryId ?? connection.id,
      });
      for (; unacked > 0; unacked -= 1) ack(id);
      const queued = pending.get(id) ?? [];
      pending.delete(id);
      for (const event of queued) handleEvent(event);
      return id;
    },

    /** Closes the tab (and so the session). */
    close(id: string) {
      tabs.closeSession(id);
    },

    /** The view measures `size` (CSS pixels); the desktop follows (debounced in Rust). */
    resize(id: string, size: RdpSize) {
      if (size.width <= 0 || size.height <= 0) return;
      const previous = viewSizes.get(id);
      lastSize = size;
      if (previous?.width === size.width && previous.height === size.height) return;
      viewSizes.set(id, size);
      if (states[id]?.status.kind === 'disconnected') return;
      void api.rdpResize(id, size.width, size.height).catch(() => {});
    },

    /** Canvas input; dropped unless the session is connected. */
    input(id: string, events: RdpInput[]) {
      if (events.length === 0 || states[id]?.status.kind !== 'connected') return;
      void api.rdpInput(id, events).catch(() => {});
    },

    /** The canvas lost focus (or an app shortcut took the key): release every held key. */
    releaseAllKeys(id: string) {
      if (!(id in states)) return;
      void api.rdpReleaseAllKeys(id).catch(() => {});
    },

    /** Starts (focus) or stops (blur) the system keyboard capture; whether keys now bypass the canvas. */
    async captureKeyboard(id: string, active: boolean): Promise<boolean> {
      if (!(id in states)) return false;
      try {
        const result = await api.rdpCaptureKeyboard(id, active);
        return result.active && result.mode === 'grab';
      } catch {
        return false;
      }
    },

    /** Connects an ended session again in its tab; a failure stays in the exit bar. */
    async reconnect(id: string) {
      const state = states[id];
      if (!state || state.status.kind !== 'disconnected') return;
      set({ ...state, status: { kind: 'connecting' }, typing: false });
      const size = sizeOf(id);
      try {
        await api.rdpReconnect(id, size.width, size.height);
      } catch (error) {
        const current = states[id];
        if (current) set(disconnected(current, errorMessage(error)));
      }
    },

    /**
     * "Type clipboard" into session `id` (Swift `typeClipboardIntoActiveSession`): above 2000 characters
     * it asks first; no text and a busy session show a toast.
     */
    async typeClipboard(id: string) {
      const state = states[id];
      if (!state || state.status.kind !== 'connected' || state.typing) return;
      const run = async (confirmed: boolean) => {
        const result = await api.rdpTypeClipboard(id, confirmed);
        switch (result.status) {
          case 'typing': {
            const current = states[id];
            if (current) set({ ...current, typing: true });
            return;
          }
          case 'needsConfirmation': {
            const ok = await dialogs.confirm({
              title: t('Type a long text?'),
              message: t(
                '%lld characters, typing takes about %lld s. Continue?',
                result.chars,
                result.seconds,
              ),
              confirmLabel: t('Type'),
            });
            if (ok && states[id]?.status.kind === 'connected') await run(true);
            return;
          }
          case 'empty':
            toast.show(t("There's no text on the clipboard"));
            return;
          case 'busy':
            toast.show(t("Can't type now: the session is busy or not connected"));
            return;
        }
      };
      try {
        await run(false);
      } catch (error) {
        alerts.show(errorMessage(error));
      }
    },

    /** Cancels the session's download of remote clipboard files. */
    cancelFileTransfer(id: string) {
      if (!states[id]?.transfer) return;
      void api.rdpCancelFileTransfer(id).catch(() => {});
    },

    /** Keeps the automatic tab colours in line with the connections. */
    syncConnections(connections: readonly Pick<ConnectionDto, 'id' | 'color'>[]) {
      // A lookup built per call.
      // eslint-disable-next-line svelte/prefer-svelte-reactivity
      const colors = new Map(connections.map((c) => [c.id, c.color]));
      for (const state of Object.values(states)) {
        if (colors.has(state.connectionId)) {
          tabs.updateSession(state.id, { color: colors.get(state.connectionId) ?? null });
        }
      }
    },
  };
  return store;
}

export type RdpSessionsStore = ReturnType<typeof createRdpSessionsStore>;

/** The app-wide instance. */
export const rdpSessions = createRdpSessionsStore();
