// An embedded SSH session's state as the UI shows it, and how `ssh-event`s change it (Swift
// `TerminalSession` status/title/recording, `AppState.recordingEnded`, `SessionTabs.closesTab`). Rust is
// the source of truth for the title, alternate screen, bracketed paste, root and recording.

import { i18n, t, tn } from '../i18n/index.svelte';
import type { ChangedHostKey, RecordOutcome, RecordPhase, SshEventDto } from '../types';

export type SessionStatus =
  | { kind: 'running' }
  | {
      kind: 'exited';
      /** The normalised exit code; `null` when unknown (signal). */
      code: number | null;
      authFailed: boolean;
      /** A failed reconnect's message (no exit event arrives then). */
      error: string | null;
      /** `ssh` refused a changed host key: what to offer replacing. */
      hostKeyChanged?: ChangedHostKey;
    };

export interface TerminalSessionState {
  /** The Rust session id; also the tab id. */
  id: string;
  connectionId: string;
  connectionName: string;
  /** The automatic tab title: the connection name until the remote sets a title. */
  title: string;
  altScreen: boolean;
  bracketedPaste: boolean;
  root: boolean;
  recording: RecordPhase;
  status: SessionStatus;
}

export function newSessionState(
  id: string,
  connectionId: string,
  connectionName: string,
): TerminalSessionState {
  return {
    id,
    connectionId,
    connectionName,
    title: connectionName,
    altScreen: false,
    bracketedPaste: false,
    root: false,
    recording: 'idle',
    status: { kind: 'running' },
  };
}

export type SessionEffect =
  /** Exit 0: close the tab. */
  { kind: 'close' } | { kind: 'toast'; message: string };

export interface Reduced {
  state: TerminalSessionState;
  effect: SessionEffect | null;
}

/** Applies one `ssh-event` (its `sessionId` already matched). */
export function applySshEvent(state: TerminalSessionState, event: SshEventDto): Reduced {
  switch (event.kind) {
    case 'title': {
      const title = event.title.trim();
      return { state: title ? { ...state, title } : state, effect: null };
    }
    case 'altScreen':
      return { state: { ...state, altScreen: event.active }, effect: null };
    case 'bracketedPaste':
      return { state: { ...state, bracketedPaste: event.enabled }, effect: null };
    case 'root':
      return { state: { ...state, root: event.root }, effect: null };
    case 'recording':
      return { state: { ...state, recording: event.phase }, effect: null };
    case 'recorded':
      return {
        state: { ...state, recording: 'idle' },
        effect: { kind: 'toast', message: recordedMessage(event.outcome, event.text) },
      };
    case 'exit': {
      if (event.code === 0) return { state: exited(state, 0, false, null), effect: { kind: 'close' } };
      return {
        state: exited(state, event.code, event.authFailed, null, event.hostKeyChanged),
        effect: null,
      };
    }
  }
}

/** The session ended (or a reconnect failed): not root any more, no recording. */
export function exited(
  state: TerminalSessionState,
  code: number | null,
  authFailed: boolean,
  error: string | null,
  hostKeyChanged?: ChangedHostKey,
): TerminalSessionState {
  return {
    ...state,
    root: false,
    recording: 'idle',
    altScreen: false,
    status: { kind: 'exited', code, authFailed, error, ...(hostKeyChanged ? { hostKeyChanged } : {}) },
  };
}

/** Back to running after a reconnect started. */
export function restarted(state: TerminalSessionState): TerminalSessionState {
  return {
    ...state,
    status: { kind: 'running' },
    recording: 'idle',
    altScreen: false,
    bracketedPaste: false,
  };
}

/** The exit bar's text. */
export function exitText(status: SessionStatus): string {
  if (status.kind !== 'exited') return '';
  if (status.error) return status.error;
  if (status.authFailed) return t('Authentication failed');
  if (status.hostKeyChanged) return t('Host key verification failed');
  return status.code === null ? t('Connection closed') : t('Connection closed (code: %d)', status.code);
}

/** Swift `ByteCountFormatter` (file style): decimal units. */
export function formatBytes(bytes: number, locale: string): string {
  const units = ['byte', 'kilobyte', 'megabyte', 'gigabyte'] as const;
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return new Intl.NumberFormat(locale, {
    style: 'unit',
    unit: units[unit],
    unitDisplay: 'short',
    maximumFractionDigits: unit === 0 ? 0 : 1,
  }).format(value);
}

/** The toast after "copy next output" (Swift `AppState.recordingEnded`). */
export function recordedMessage(outcome: RecordOutcome, text: string, locale: string = i18n.locale): string {
  switch (outcome) {
    case 'copied': {
      const lines = text.split('\n').length;
      const size = formatBytes(new TextEncoder().encode(text).length, locale);
      return tn('Copied %lld lines (%@)', lines, size);
    }
    case 'empty':
      return t('The command printed nothing');
    case 'alternateScreen':
      return t('Interactive program – recording stopped. Try cat or --no-pager.');
    case 'tooLarge':
      return t('Output larger than 10 MB – recording stopped');
  }
}
