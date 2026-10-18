// The embedded SSH sessions (Swift `AppState` session lifecycle): tab id = Rust session id → state, plus
// the session's long-lived terminal. Opening adds a tab; closing the tab (any way) ends the session.
// `ssh-event`s update the state; exit 0 closes the tab, any other exit shows the exit bar (and, when `ssh`
// refused a changed host key, the host key dialog offering to replace it).
//
// A local terminal (`openLocal`, or `open` with `LOCAL_TERMINAL`) is a session of the same kind running
// this machine's shell; its tabs are named "Local Terminal", "Local Terminal 2"…
//
// A session can instead be a pane of a split tab (`open(…, { pane: true })` or `adopt`): it has no tab of its
// own, and the split store (the `PaneHost`) hears about its changes, closes it and picks its broadcast
// targets.

import { Channel } from '@tauri-apps/api/core';
import { createApi, onSshEvent, tauriBridge, type Api, type Listen, type Unlisten } from '../api';
import { errorMessage } from '../errors';
import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
import { app } from '../stores/app.svelte';
import { settings } from '../stores/settings.svelte';
import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
import { tabs as defaultTabs, type TabsStore } from '../tabs.svelte';
import type { HostKeyPrompts } from '../forwards/hostKeys.svelte';
import type { ConnectionDto, HostKeyQuestion, Platform, ShortcutDto, SshEventDto } from '../types';
import { LOCAL_TERMINAL, isLocalTerminal, localTerminalName } from './local';
import { applySshEvent, exited, newSessionState, restarted, type TerminalSessionState } from './model';
import type { TerminalFactory, TerminalHandle, TerminalSize } from './xterm';

/** What receives a session's output (a Tauri `Channel`); tests pass plain objects. */
export interface OutputChannel {
  onmessage: (data: unknown) => void;
}

export interface SessionsDeps {
  api: Api;
  listen: Listen;
  tabs: TabsStore;
  toast: Pick<ToastStore, 'show'>;
  alerts: Pick<AlertStore, 'show'>;
  /** Creates a session's terminal; by default xterm, loaded on first use. */
  createTerminal: TerminalFactory;
  createChannel: () => OutputChannel;
  platform: () => Platform;
  shortcuts: () => readonly ShortcutDto[];
}

/** The owner of pane sessions (the split store). */
export interface PaneHost {
  /** Panes that also receive input typed into pane `id` (empty: it doesn't broadcast). */
  targets(id: string): string[];
  /** Pane `id` should close (exit 0, or its Close button). */
  closePane(id: string): void;
  /** Pane `id`'s state changed (title, root, status). */
  changed(id: string): void;
  /** The focused pane of tab `tabId` when it is a split. */
  focusedPane(tabId: string): string | null;
}

export interface OpenOptions {
  /** Open as a split pane: no tab of its own. */
  pane?: boolean;
}

/** Events kept for a session whose `open_ssh_session` hasn't answered yet. */
const MAX_PENDING = 64;

/** Raw channel bytes as a `Uint8Array`. */
export function toBytes(data: unknown): Uint8Array {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (Array.isArray(data)) return Uint8Array.from(data as number[]);
  return new Uint8Array();
}

async function loadXterm(): Promise<TerminalFactory> {
  return (await import('./xterm')).createXtermTerminal;
}

export function createSessionsStore(deps: Partial<SessionsDeps> = {}) {
  const api = deps.api ?? createApi(tauriBridge.invoke);
  const listen = deps.listen ?? tauriBridge.listen;
  const tabs = deps.tabs ?? defaultTabs;
  const toast = deps.toast ?? defaultToast;
  const alerts = deps.alerts ?? defaultAlerts;
  const createChannel = deps.createChannel ?? (() => new Channel<ArrayBuffer>() as unknown as OutputChannel);
  const platform = deps.platform ?? (() => 'macos' as Platform);
  const shortcuts = deps.shortcuts ?? (() => []);

  let states = $state<Record<string, TerminalSessionState>>({});
  // Not state: terminals and queued events don't render (`states` does).
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const handles = new Map<string, TerminalHandle>();
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const pending = new Map<string, SshEventDto[]>();
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const panes = new Set<string>();
  let host: PaneHost | null = null;
  // Sessions whose changed host key dialog was answered, until their next run.
  let answeredHostKeys = $state<Record<string, true>>({});
  let lastSize: TerminalSize = { cols: 80, rows: 24 };
  let unlisten: Unlisten | null = null;

  function set(state: TerminalSessionState) {
    states = { ...states, [state.id]: state };
    if (panes.has(state.id)) host?.changed(state.id);
    else tabs.updateSession(state.id, { title: state.title, root: state.root });
  }

  /** Closes a session's tab, or asks the split to close the pane. */
  function closeSession(id: string) {
    if (!panes.has(id)) tabs.closeSession(id);
    else if (host) host.closePane(id);
    else end(id);
  }

  /** Ends a pane session (the split already dropped it). */
  function end(id: string) {
    panes.delete(id);
    forget(id);
  }

  const isRunning = (id: string) => states[id]?.status.kind === 'running';

  /** Ends a session whose tab closed. */
  function forget(id: string) {
    if (!(id in states)) return;
    const rest = { ...states };
    delete rest[id];
    states = rest;
    forgetHostKeyAnswer(id);
    pending.delete(id);
    handles.get(id)?.dispose();
    handles.delete(id);
    void api.sshClose(id).catch(() => {});
  }

  function forgetHostKeyAnswer(id: string) {
    if (!(id in answeredHostKeys)) return;
    const rest = { ...answeredHostKeys };
    delete rest[id];
    answeredHostKeys = rest;
  }

  /**
   * The changed host keys `ssh` refused, for the host key dialog (`requestId` is the session id): one at
   * a time, until answered. "Replace key" replaces the stored key and reconnects; Cancel leaves the exit
   * bar (a Reconnect that fails the same way asks again).
   */
  const hostKeyPrompts: HostKeyPrompts = {
    get current(): HostKeyQuestion | null {
      for (const state of Object.values(states)) {
        const status = state.status;
        if (status.kind !== 'exited' || !status.hostKeyChanged || answeredHostKeys[state.id]) continue;
        const change = status.hostKeyChanged;
        return {
          requestId: state.id,
          // `ssh` printed the stored name, `[host]:port` included.
          host: change.host,
          port: 22,
          keyType: change.keyType,
          fingerprint: change.fingerprint,
          changed: { file: change.file, line: change.line },
        };
      }
      return null;
    },
    async answer(accept: boolean) {
      const id = hostKeyPrompts.current?.requestId;
      if (!id) return;
      answeredHostKeys = { ...answeredHostKeys, [id]: true };
      if (!accept) return;
      try {
        await api.sshReplaceHostKey(id);
      } catch (error) {
        alerts.show(errorMessage(error));
        return;
      }
      await store.reconnect(id);
    },
  };

  const offClosed = tabs.onSessionClosed(forget);

  function handleEvent(event: SshEventDto) {
    const state = states[event.sessionId];
    if (!state) {
      const queue = pending.get(event.sessionId) ?? [];
      if (queue.length < MAX_PENDING) pending.set(event.sessionId, [...queue, event]);
      return;
    }
    const { state: next, effect } = applySshEvent(state, event);
    set(next);
    if (effect?.kind === 'close') closeSession(next.id);
    else if (effect?.kind === 'toast') toast.show(effect.message);
  }

  const resolveFactory = async (): Promise<TerminalFactory> => deps.createTerminal ?? (await loadXterm());

  const store = {
    /** The state of every open session, by tab id. */
    get states(): Readonly<Record<string, TerminalSessionState>> {
      return states;
    },
    state(id: string | null): TerminalSessionState | undefined {
      return id === null ? undefined : states[id];
    },
    has(id: string): boolean {
      return id in states;
    },
    handle(id: string): TerminalHandle | undefined {
      return handles.get(id);
    },
    /** The active tab's session, if it is a terminal; on a split tab its focused pane. */
    get active(): TerminalSessionState | undefined {
      const active = tabs.active;
      if (active === null) return undefined;
      return states[active] ?? store.state(host?.focusedPane(active) ?? null);
    },
    /** Whether `id` is a split pane (has no tab of its own). */
    isPane(id: string): boolean {
      return panes.has(id);
    },
    /** Makes the split store the owner of pane sessions. */
    setPaneHost(next: PaneHost | null) {
      host = next;
    },
    /** Turns a tab's session into a pane (the caller replaces the tab); the terminal and process stay. */
    adopt(id: string) {
      if (id in states) panes.add(id);
    },
    /** Ends a pane's session. */
    end,
    /** The changed host key prompts, for `HostKeyDialog`. */
    hostKeyPrompts,
    /** Live sessions (the quit confirmation counts them in Rust too). */
    get runningCount(): number {
      return Object.values(states).filter((s) => s.status.kind === 'running').length;
    },

    /** Subscribes to `ssh-event`. */
    async start() {
      if (!unlisten) unlisten = await onSshEvent(listen, handleEvent);
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
    /** Applies an event (exposed for tests and the listener). */
    handleEvent,

    /** Opens an embedded session for `connection` in a new tab; rejects with the `CommandError`. */
    async open(
      connection: Pick<ConnectionDto, 'id' | 'name' | 'type' | 'color'>,
      options: OpenOptions = {},
    ): Promise<string> {
      const factory = await resolveFactory();
      let sessionId: string | null = null;
      const handle = factory({
        initialSize: lastSize,
        onInput: (data) => {
          if (!sessionId || !isRunning(sessionId)) return;
          const targets = host && panes.has(sessionId) ? host.targets(sessionId) : [];
          const sent =
            targets.length > 0
              ? api.sshBroadcastInput(sessionId, targets, data)
              : api.sshInput(sessionId, data);
          void sent.catch(() => {});
        },
        onResize: (size) => {
          lastSize = size;
          if (sessionId && isRunning(sessionId)) {
            void api.sshResize(sessionId, size.cols, size.rows).catch(() => {});
          }
        },
        keyContext: () => ({
          platform: platform(),
          shortcuts: shortcuts(),
          recording: sessionId !== null && (states[sessionId]?.recording ?? 'idle') !== 'idle',
        }),
        cancelRecording: () => {
          if (sessionId) void store.cancelRecording(sessionId);
        },
        copy: (text) => void api.copyText(text).catch((e: unknown) => alerts.show(errorMessage(e))),
        paste: () => api.terminalPasteText().catch(() => null),
        openLink: (url) => void api.terminalOpenUrl(url).catch((e: unknown) => alerts.show(errorMessage(e))),
      });
      const channel = createChannel();
      channel.onmessage = (data) => handle.write(toBytes(data));
      const size = handle.size();
      const local = isLocalTerminal(connection.id);
      try {
        ({ sessionId } = local
          ? await api.openLocalTerminal(size.cols, size.rows, channel)
          : await api.openSshSession(connection.id, size.cols, size.rows, channel));
      } catch (error) {
        handle.dispose();
        throw error;
      }
      const id = sessionId;
      const name = local
        ? localTerminalName(
            Object.values(states)
              .filter((s) => isLocalTerminal(s.connectionId))
              .map((s) => s.connectionName),
          )
        : connection.name;
      handles.set(id, handle);
      states = { ...states, [id]: newSessionState(id, connection.id, name) };
      if (options.pane) panes.add(id);
      else tabs.openSession({ id, title: name, type: connection.type, color: connection.color });
      const queued = pending.get(id) ?? [];
      pending.delete(id);
      for (const event of queued) handleEvent(event);
      return id;
    },

    /** Opens a local terminal (this machine's shell) in a new tab; rejects with the `CommandError`. */
    openLocal(options: OpenOptions = {}): Promise<string> {
      return store.open(LOCAL_TERMINAL, options);
    },

    /** Closes the tab (and so the session), or the pane. */
    close(id: string) {
      closeSession(id);
    },

    /** Starts an exited session again in its tab; a failure stays in the exit bar. */
    async reconnect(id: string) {
      const state = states[id];
      if (!state || state.status.kind !== 'exited') return;
      forgetHostKeyAnswer(id);
      set(restarted(state));
      const size = handles.get(id)?.size() ?? lastSize;
      handles.get(id)?.focus();
      try {
        await api.sshReconnect(id, size.cols, size.rows);
      } catch (error) {
        const current = states[id];
        if (current) set(exited(current, null, false, errorMessage(error)));
      }
    },

    /** "Clear terminal": screen and scrollback locally, then Ctrl+L so the shell redraws its prompt. */
    async clear(id: string) {
      if (!(id in states)) return;
      let action;
      try {
        action = await api.sshClearDecision(id);
      } catch {
        return;
      }
      if (action === 'none') return;
      handles.get(id)?.clearLocal();
      if (action === 'localAndRedraw') await api.sshInput(id, '\x0c').catch(() => {});
    },

    /** The record button: arms, cancels or finishes "copy next output". */
    async toggleRecording(id: string) {
      if (!isRunning(id)) return;
      try {
        const phase = await api.sshRecordNextOutput(id);
        const state = states[id];
        if (state) set({ ...state, recording: phase });
      } catch (error) {
        alerts.show(errorMessage(error));
      }
      handles.get(id)?.focus();
    },

    /** Esc or Cancel while recording. */
    async cancelRecording(id: string) {
      const state = states[id];
      if (!state || state.recording === 'idle') return;
      set({ ...state, recording: 'idle' });
      await api.sshRecordCancel(id).catch(() => {});
    },

    /** Keeps the automatic tab colours in line with the connections (Swift `connectionColor`). */
    syncConnections(connections: readonly Pick<ConnectionDto, 'id' | 'color'>[]) {
      // A lookup built per call.
      // eslint-disable-next-line svelte/prefer-svelte-reactivity
      const colors = new Map(connections.map((c) => [c.id, c.color]));
      for (const state of Object.values(states)) {
        if (panes.has(state.id)) continue;
        if (colors.has(state.connectionId)) {
          tabs.updateSession(state.id, { color: colors.get(state.connectionId) ?? null });
        }
      }
    },
  };
  return store;
}

export type SessionsStore = ReturnType<typeof createSessionsStore>;

/** The app-wide instance. */
export const sessions = createSessionsStore({
  platform: () => app.platform,
  shortcuts: () => settings.shortcuts,
});
