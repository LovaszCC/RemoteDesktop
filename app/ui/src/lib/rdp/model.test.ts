import { describe, expect, it } from 'vitest';
import type { RdpEvent } from '../types';
import {
  applyRdpEvent,
  disconnected,
  initialViewSize,
  newRdpState,
  pointerCursor,
  rdpExitText,
  transferBytes,
  transferFraction,
} from './model';

const state = newRdpState('S1', 'c1', 'Desk');
const event = (e: RdpEvent) => ({ sessionId: 'S1', ...e });

describe('rdp session model', () => {
  it('starts connecting', () => {
    expect(state).toEqual({
      id: 'S1',
      connectionId: 'c1',
      connectionName: 'Desk',
      status: { kind: 'connecting' },
      desktop: null,
      typing: false,
      transfer: null,
      cursor: 'default',
    });
  });

  it('connects, resizes and reconnects', () => {
    const connected = applyRdpEvent(state, event({ kind: 'connected', width: 1280, height: 800 }));
    expect(connected.state.status).toEqual({ kind: 'connected' });
    expect(connected.state.desktop).toEqual({ width: 1280, height: 800 });
    expect(connected.effect).toBeNull();
    const resized = applyRdpEvent(connected.state, event({ kind: 'resized', width: 1000, height: 700 }));
    expect(resized.state.desktop).toEqual({ width: 1000, height: 700 });
    expect(resized.state.status.kind).toBe('connected');
    const typing = { ...resized.state, typing: true };
    const reconnecting = applyRdpEvent(typing, event({ kind: 'reconnecting', attempt: 2 }));
    expect(reconnecting.state.status).toEqual({ kind: 'reconnecting', attempt: 2 });
    expect(reconnecting.state.typing).toBe(false);
  });

  it('closes the tab on a clean disconnect and keeps it otherwise', () => {
    const clean = applyRdpEvent(
      state,
      event({ kind: 'disconnected', clean: true, code: 12, message: 'bye' }),
    );
    expect(clean.effect).toEqual({ kind: 'close' });
    const dropped = applyRdpEvent(
      { ...state, typing: true },
      event({ kind: 'disconnected', clean: false, code: 0, message: '' }),
    );
    expect(dropped.effect).toBeNull();
    expect(dropped.state.status).toEqual({ kind: 'disconnected', clean: false, code: 0, message: '' });
    expect(dropped.state.typing).toBe(false);
  });

  it('toasts the remote clipboard and an interrupted typing', () => {
    expect(applyRdpEvent(state, event({ kind: 'remoteClipboard', chars: 1 })).effect).toEqual({
      kind: 'toast',
      message: 'Remote clipboard copied (1 character)',
    });
    expect(applyRdpEvent(state, event({ kind: 'remoteClipboard', chars: 5 })).effect).toEqual({
      kind: 'toast',
      message: 'Remote clipboard copied (5 characters)',
    });
    const typing = { ...state, typing: true };
    const finished = applyRdpEvent(typing, event({ kind: 'typed', finished: true }));
    expect(finished).toEqual({ state, effect: null });
    expect(applyRdpEvent(typing, event({ kind: 'typed', finished: false })).effect).toEqual({
      kind: 'toast',
      message: 'Typing was interrupted',
    });
  });

  it('tracks file downloads and words their end like Swift', () => {
    const progress = applyRdpEvent(state, event({ kind: 'fileProgress', done: 1500, total: 3_000_000 }));
    expect(progress.state.transfer).toEqual({ done: 1500, total: 3_000_000 });
    expect(progress.effect).toBeNull();
    const downloading = progress.state;
    expect(applyRdpEvent(downloading, event({ kind: 'filesReady', count: 1 }))).toEqual({
      state,
      effect: { kind: 'toast', message: '1 remote file copied to the clipboard' },
    });
    expect(applyRdpEvent(downloading, event({ kind: 'filesReady', count: 3 })).effect).toEqual({
      kind: 'toast',
      message: '3 remote files copied to the clipboard',
    });
    expect(applyRdpEvent(downloading, event({ kind: 'fileTransferFailed', message: 'disk full' }))).toEqual({
      state,
      effect: { kind: 'toast', message: 'Copying files failed: disk full' },
    });
    expect(applyRdpEvent(downloading, event({ kind: 'fileTransferCancelled' }))).toEqual({
      state,
      effect: null,
    });
    expect(applyRdpEvent(downloading, event({ kind: 'reconnecting', attempt: 1 })).state.transfer).toBeNull();
    expect(disconnected(downloading, '').transfer).toBeNull();
  });

  it('shows the remote pointer as the cursor', () => {
    const shaped = applyRdpEvent(
      state,
      event({ kind: 'pointerShape', png: 'iVBO', hotspotX: 3, hotspotY: 7 }),
    );
    expect(shaped).toEqual({
      state: { ...state, cursor: 'url("data:image/png;base64,iVBO") 3 7, default' },
      effect: null,
    });
    expect(pointerCursor('AA', 0, 0)).toBe('url("data:image/png;base64,AA") 0 0, default');
    expect(applyRdpEvent(shaped.state, event({ kind: 'pointerHidden' })).state.cursor).toBe('none');
    expect(applyRdpEvent(shaped.state, event({ kind: 'reconnecting', attempt: 1 })).state.cursor).toBe(
      'default',
    );
    expect(disconnected(shaped.state, '').cursor).toBe('default');
  });

  it('formats the download progress', () => {
    expect(transferFraction({ done: 1, total: 4 })).toBe(0.25);
    expect(transferFraction({ done: 9, total: 4 })).toBe(1);
    expect(transferFraction({ done: 0, total: 0 })).toBeNull();
    expect(transferBytes({ done: 1500, total: 3_000_000 }, 'en')).toBe('1.5 KB of 3 MB');
    expect(transferBytes({ done: 12, total: 0 }, 'en')).toBe('12 B');
  });

  it('words the exit bar like Swift', () => {
    expect(rdpExitText({ kind: 'connected' })).toBe('');
    expect(rdpExitText(disconnected(state, '').status)).toBe(
      'The connection dropped unexpectedly (the server gave no reason)',
    );
    expect(rdpExitText(disconnected(state, 'Logged off by the administrator').status)).toBe(
      'Logged off by the administrator',
    );
  });

  it('sizes a new session from the window, at least 800×600 with an even width', () => {
    expect(initialViewSize({ innerWidth: 1600, innerHeight: 1000 })).toEqual({ width: 1320, height: 880 });
    expect(initialViewSize({ innerWidth: 1581, innerHeight: 900 })).toEqual({ width: 1300, height: 780 });
    expect(initialViewSize({ innerWidth: 500, innerHeight: 400 })).toEqual({ width: 800, height: 600 });
  });
});
