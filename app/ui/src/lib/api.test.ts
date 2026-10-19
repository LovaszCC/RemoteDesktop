import { describe, expect, it, vi } from 'vitest';
import { createApi, isCommandError, toCommandError } from './api';

describe('api', () => {
  it('passes the command name and camelCase arguments', async () => {
    const invoke = vi.fn().mockResolvedValue('id-1');
    const api = createApi(invoke);
    await api.saveEntry(null, { name: 'Box', host: 'box' }, []);
    expect(invoke).toHaveBeenLastCalledWith('save_entry', {
      id: null,
      entry: { name: 'Box', host: 'box' },
      connections: [],
    });
    await api.changePassword('old', 'new');
    expect(invoke).toHaveBeenLastCalledWith('change_password', { current: 'old', new: 'new' });
    await api.moveItems(['c:1'], null);
    expect(invoke).toHaveBeenLastCalledWith('move_items', { tags: ['c:1'], targetFolderId: null });
    await api.setCredential('k:1', { connectionIds: ['1'] });
    expect(invoke).toHaveBeenLastCalledWith('set_credential', {
      credentialId: 'k:1',
      tags: null,
      connectionIds: ['1'],
    });
    await api.resolveDbLock('readOnly');
    expect(invoke).toHaveBeenLastCalledWith('resolve_db_lock', { choice: 'readOnly' });
    await api.pickImportFile('winbox');
    expect(invoke).toHaveBeenLastCalledWith('pick_import_file', { kind: 'winbox' });
    await api.importPreview('rdm', '/tmp/x.rdm');
    expect(invoke).toHaveBeenLastCalledWith('import_preview', { kind: 'rdm', path: '/tmp/x.rdm' });
    await api.importCommit('rdm', '/tmp/x.rdm', ['c0', 'k1']);
    expect(invoke).toHaveBeenLastCalledWith('import_commit', {
      kind: 'rdm',
      path: '/tmp/x.rdm',
      selected: ['c0', 'k1'],
    });
    await api.appStatus();
    expect(invoke).toHaveBeenLastCalledWith('app_status', undefined);
  });

  it('wraps the embedded RDP commands', async () => {
    const invoke = vi.fn().mockResolvedValue(null);
    const api = createApi(invoke);
    const channel = {};
    await api.openRdpSession('c1', 1280, 800, channel);
    expect(invoke).toHaveBeenLastCalledWith('open_rdp_session', {
      connectionId: 'c1',
      width: 1280,
      height: 800,
      onFrame: channel,
    });
    await api.rdpAckFrame('S');
    expect(invoke).toHaveBeenLastCalledWith('rdp_ack_frame', { sessionId: 'S' });
    await api.rdpInput('S', [{ type: 'mouseMove', x: 1, y: 2 }]);
    expect(invoke).toHaveBeenLastCalledWith('rdp_input', {
      sessionId: 'S',
      events: [{ type: 'mouseMove', x: 1, y: 2 }],
    });
    await api.rdpReleaseAllKeys('S');
    expect(invoke).toHaveBeenLastCalledWith('rdp_release_all_keys', { sessionId: 'S' });
    await api.rdpResize('S', 1000, 700);
    expect(invoke).toHaveBeenLastCalledWith('rdp_resize', { sessionId: 'S', width: 1000, height: 700 });
    await api.rdpClose('S');
    expect(invoke).toHaveBeenLastCalledWith('rdp_close', { sessionId: 'S' });
    await api.rdpReconnect('S', 1000, 700);
    expect(invoke).toHaveBeenLastCalledWith('rdp_reconnect', { sessionId: 'S', width: 1000, height: 700 });
    await api.rdpTypeClipboard('S', true);
    expect(invoke).toHaveBeenLastCalledWith('rdp_type_clipboard', { sessionId: 'S', confirmed: true });
    await api.rdpCertAnswer('R', 'acceptOnce');
    expect(invoke).toHaveBeenLastCalledWith('rdp_cert_answer', { requestId: 'R', choice: 'acceptOnce' });
    await api.rdpCredentialsAnswer('R', null);
    expect(invoke).toHaveBeenLastCalledWith('rdp_credentials_answer', { requestId: 'R', answer: null });
    await api.rdpCaptureKeyboard('S', true);
    expect(invoke).toHaveBeenLastCalledWith('rdp_capture_keyboard', { sessionId: 'S', active: true });
    await api.rdpEngineInfo();
    expect(invoke).toHaveBeenLastCalledWith('rdp_engine_info', undefined);
  });

  it('rejects with a CommandError', async () => {
    const error = { code: 'wrongPassword', message: 'Wrong password', detail: null };
    await expect(createApi(vi.fn().mockRejectedValue(error)).unlock('x', true)).rejects.toEqual(error);
    await expect(createApi(vi.fn().mockRejectedValue('bridge down')).lock()).rejects.toEqual({
      code: 'internal',
      message: 'The command failed.',
      detail: 'bridge down',
    });
  });

  it('normalizes thrown values', () => {
    expect(isCommandError({ code: 'a', message: 'b' })).toBe(true);
    expect(isCommandError('nope')).toBe(false);
    expect(toCommandError({ code: 'a', message: 'b' })).toEqual({ code: 'a', message: 'b', detail: null });
    expect(toCommandError(new Error('boom')).detail).toBe('boom');
  });
});
