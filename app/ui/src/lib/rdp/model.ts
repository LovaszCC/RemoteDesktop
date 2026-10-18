// An embedded RDP session's state as the UI shows it, and how `rdp-event`s change it (Swift `RDPSession`
// status, `AppState.openRDPSession` onEnd, `typeClipboardIntoActiveSession` and the file clipboard's
// progress panel and messages). A clean disconnect (logoff,
// cancelled sign-in) closes the tab; any other ends in the exit bar with the server's reason and Reconnect.

import { formatSize } from '../files/model';
import { t, tn } from '../i18n/index.svelte';
import type { RdpEventDto } from '../types';

export type RdpStatus =
  | { kind: 'connecting' }
  | { kind: 'connected' }
  | { kind: 'reconnecting'; attempt: number }
  /** `message`: the server's reason, empty when it gave none (or a failed reconnect's error). */
  | { kind: 'disconnected'; clean: boolean; code: number; message: string };

export interface RdpSize {
  width: number;
  height: number;
}

export interface RdpSessionState {
  /** The Rust session id; also the tab id. */
  id: string;
  connectionId: string;
  connectionName: string;
  status: RdpStatus;
  /** The remote desktop's size once connected (it lags the view while a resize is pending). */
  desktop: RdpSize | null;
  /** "Type clipboard" is running. */
  typing: boolean;
  /** Files from the remote clipboard are downloading. */
  transfer: RdpTransfer | null;
  /** The canvas's CSS cursor: the remote pointer's shape (the server doesn't draw it into the desktop). */
  cursor: string;
}

/** A running download of remote clipboard files, in bytes. */
export interface RdpTransfer {
  done: number;
  total: number;
}

export function newRdpState(id: string, connectionId: string, connectionName: string): RdpSessionState {
  return {
    id,
    connectionId,
    connectionName,
    status: { kind: 'connecting' },
    desktop: null,
    typing: false,
    transfer: null,
    cursor: 'default',
  };
}

export type RdpEffect = { kind: 'close' } | { kind: 'toast'; message: string };

export interface RdpReduced {
  state: RdpSessionState;
  effect: RdpEffect | null;
}

/** Applies one `rdp-event` (its `sessionId` already matched). */
export function applyRdpEvent(state: RdpSessionState, event: RdpEventDto): RdpReduced {
  switch (event.kind) {
    case 'connected':
      return {
        state: {
          ...state,
          status: { kind: 'connected' },
          desktop: { width: event.width, height: event.height },
        },
        effect: null,
      };
    case 'resized':
      return { state: { ...state, desktop: { width: event.width, height: event.height } }, effect: null };
    case 'reconnecting':
      return {
        state: {
          ...state,
          status: { kind: 'reconnecting', attempt: event.attempt },
          typing: false,
          transfer: null,
          cursor: 'default',
        },
        effect: null,
      };
    case 'disconnected': {
      const next = disconnected(state, event.message, event.clean, event.code);
      return { state: next, effect: event.clean ? { kind: 'close' } : null };
    }
    case 'remoteClipboard':
      return {
        state,
        effect: { kind: 'toast', message: tn('Remote clipboard copied (%lld characters)', event.chars) },
      };
    case 'typed':
      return {
        state: { ...state, typing: false },
        effect: event.finished ? null : { kind: 'toast', message: t('Typing was interrupted') },
      };
    case 'fileProgress':
      return { state: { ...state, transfer: { done: event.done, total: event.total } }, effect: null };
    case 'filesReady':
      return {
        state: { ...state, transfer: null },
        effect: { kind: 'toast', message: tn('%lld remote files copied to the clipboard', event.count) },
      };
    case 'fileTransferFailed':
      return {
        state: { ...state, transfer: null },
        effect: { kind: 'toast', message: t('Copying files failed: %@', event.message) },
      };
    case 'fileTransferCancelled':
      return { state: { ...state, transfer: null }, effect: null };
    case 'pointerShape':
      return {
        state: { ...state, cursor: pointerCursor(event.png, event.hotspotX, event.hotspotY) },
        effect: null,
      };
    case 'pointerHidden':
      return { state: { ...state, cursor: 'none' }, effect: null };
  }
}

/** The CSS cursor of a remote pointer shape; the arrow when the browser can't use the image (too big). */
export function pointerCursor(png: string, hotspotX: number, hotspotY: number): string {
  return `url("data:image/png;base64,${png}") ${hotspotX} ${hotspotY}, default`;
}

/** How far a download is (0…1), or `null` while the total is unknown. */
export function transferFraction(transfer: RdpTransfer): number | null {
  return transfer.total > 0 ? Math.min(1, transfer.done / transfer.total) : null;
}

/** "1.2 MB of 3 MB" (Swift `ByteCountFormatter`). */
export function transferBytes(transfer: RdpTransfer, locale: string): string {
  const done = formatSize(transfer.done, locale);
  return transfer.total > 0 ? t('%@ of %@', done, formatSize(transfer.total, locale)) : done;
}

/** The session ended (or a reconnect failed with `message`). */
export function disconnected(
  state: RdpSessionState,
  message: string,
  clean = false,
  code = 0,
): RdpSessionState {
  return {
    ...state,
    status: { kind: 'disconnected', clean, code, message },
    typing: false,
    transfer: null,
    cursor: 'default',
  };
}

/** The exit bar's text (Swift `RDPSessionView`). */
export function rdpExitText(status: RdpStatus): string {
  if (status.kind !== 'disconnected') return '';
  return status.message || t('The connection dropped unexpectedly (the server gave no reason)');
}

/** The view size a new session starts with before the tab is laid out (Swift `RDPSession.initialSize`). */
export function initialViewSize(window: { innerWidth: number; innerHeight: number }): RdpSize {
  return {
    width: Math.max(800, Math.min(8192, Math.floor(window.innerWidth) - 280)) & ~1,
    height: Math.max(600, Math.min(8192, Math.floor(window.innerHeight) - 120)),
  };
}
