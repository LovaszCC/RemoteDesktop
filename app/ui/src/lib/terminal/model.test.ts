import { describe, expect, it } from 'vitest';
import type { SshEvent, SshEventDto } from '../types';
import {
  applySshEvent,
  exitText,
  exited,
  formatBytes,
  newSessionState,
  recordedMessage,
  restarted,
} from './model';

const base = () => newSessionState('S', 'c1', 'web01');
const ev = (event: SshEvent) => ({ sessionId: 'S', ...event }) as SshEventDto;

describe('session reducer', () => {
  it('starts running with the connection name as title', () => {
    expect(base()).toMatchObject({
      title: 'web01',
      root: false,
      recording: 'idle',
      status: { kind: 'running' },
    });
  });

  it('takes trimmed non-empty titles', () => {
    const { state } = applySshEvent(base(), ev({ kind: 'title', title: '  root@web01: ~ ' }));
    expect(state.title).toBe('root@web01: ~');
    expect(applySshEvent(state, ev({ kind: 'title', title: '   ' })).state.title).toBe('root@web01: ~');
  });

  it('mirrors alternate screen, bracketed paste, root and the recording phase', () => {
    let state = base();
    for (const event of [
      ev({ kind: 'altScreen', active: true }),
      ev({ kind: 'bracketedPaste', enabled: true }),
      ev({ kind: 'root', root: true }),
      ev({ kind: 'recording', phase: 'capturing' }),
    ]) {
      const reduced = applySshEvent(state, event);
      expect(reduced.effect).toBeNull();
      state = reduced.state;
    }
    expect(state).toMatchObject({
      altScreen: true,
      bracketedPaste: true,
      root: true,
      recording: 'capturing',
    });
  });

  it('toasts a recording result and goes idle', () => {
    const state = { ...base(), recording: 'capturing' as const };
    const reduced = applySshEvent(state, ev({ kind: 'recorded', outcome: 'copied', text: 'a\nb\nc' }));
    expect(reduced.state.recording).toBe('idle');
    expect(reduced.effect).toEqual({ kind: 'toast', message: 'Copied 3 lines (5 byte)' });
  });

  it('closes the tab on exit 0', () => {
    const reduced = applySshEvent(base(), ev({ kind: 'exit', code: 0, authFailed: false }));
    expect(reduced.effect).toEqual({ kind: 'close' });
  });

  it('keeps the tab with the exit bar on any other exit, not root and not recording any more', () => {
    const running = { ...base(), root: true, recording: 'armed' as const };
    const reduced = applySshEvent(running, ev({ kind: 'exit', code: 255, authFailed: true }));
    expect(reduced.effect).toBeNull();
    expect(reduced.state).toMatchObject({
      root: false,
      recording: 'idle',
      status: { kind: 'exited', code: 255, authFailed: true, error: null },
    });
    const signal = applySshEvent(base(), ev({ kind: 'exit', code: null, authFailed: false }));
    expect(signal.state.status).toEqual({ kind: 'exited', code: null, authFailed: false, error: null });
  });

  it('restarts after a reconnect', () => {
    const state = restarted(exited(base(), 1, false, null));
    expect(state.status).toEqual({ kind: 'running' });
  });
});

describe('exitText', () => {
  it('names the code, an authentication failure or a reconnect error', () => {
    expect(exitText({ kind: 'running' })).toBe('');
    expect(exitText({ kind: 'exited', code: 3, authFailed: false, error: null })).toBe(
      'Connection closed (code: 3)',
    );
    expect(exitText({ kind: 'exited', code: null, authFailed: false, error: null })).toBe(
      'Connection closed',
    );
    expect(exitText({ kind: 'exited', code: 255, authFailed: true, error: null })).toBe(
      'Authentication failed',
    );
    expect(exitText({ kind: 'exited', code: null, authFailed: false, error: 'No ssh' })).toBe('No ssh');
  });

  it('keeps and names a refused changed host key', () => {
    const hostKeyChanged = {
      host: '[192.0.2.5]:2221',
      keyType: 'ED25519',
      fingerprint: 'SHA256:new',
      file: '/home/u/.ssh/known_hosts',
      line: 2,
    };
    const reduced = applySshEvent(base(), ev({ kind: 'exit', code: 255, authFailed: false, hostKeyChanged }));
    expect(reduced.effect).toBeNull();
    expect(reduced.state.status).toEqual({
      kind: 'exited',
      code: 255,
      authFailed: false,
      error: null,
      hostKeyChanged,
    });
    expect(exitText(reduced.state.status)).toBe('Host key verification failed');
  });
});

describe('recording messages (Swift AppState.recordingEnded)', () => {
  it('has a text per outcome', () => {
    expect(recordedMessage('copied', 'x')).toBe('Copied 1 lines (1 byte)');
    expect(recordedMessage('empty', '')).toBe('The command printed nothing');
    expect(recordedMessage('alternateScreen', '')).toBe(
      'Interactive program – recording stopped. Try cat or --no-pager.',
    );
    expect(recordedMessage('tooLarge', '')).toBe('Output larger than 10 MB – recording stopped');
  });

  it('formats sizes with decimal units', () => {
    expect(formatBytes(999, 'en')).toBe('999 byte');
    expect(formatBytes(1500, 'en')).toBe('1.5 kB');
    expect(formatBytes(2_000_000, 'en')).toBe('2 MB');
  });
});
