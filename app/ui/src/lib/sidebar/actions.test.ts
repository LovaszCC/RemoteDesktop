import { describe, expect, it, vi } from 'vitest';
import { createApi } from '../api';
import { createAlertStore } from '../stores/alert.svelte';
import { createDialogStore } from '../stores/dialogs.svelte';
import { createEditorStore } from '../stores/editor.svelte';
import { createLibraryStore } from '../stores/library.svelte';
import { createSettingsStore } from '../stores/settings.svelte';
import { createToastStore } from '../stores/toast.svelte';
import { fakeBridge } from '../testing/bridge';
import { multiEntryLibrary, settingsDto, sidebarLibrary } from '../testing/fixtures';
import type { LaunchOutcomeDto } from '../types';
import { createSidebarActions } from './actions.svelte';

function setup(replies: Parameters<typeof fakeBridge>[0] = {}, data = sidebarLibrary()) {
  const bridge = fakeBridge({
    settings_set: (args?: Record<string, unknown>) => settingsDto(args?.patch as object),
    ...replies,
  });
  const library = createLibraryStore(bridge);
  library.replace(data);
  const deps = {
    api: createApi(bridge.invoke),
    library,
    settings: createSettingsStore({ ...bridge, onLanguage: () => {} }),
    toast: createToastStore(),
    alerts: createAlertStore(),
    editor: createEditorStore(),
    dialogs: createDialogStore(),
    sessions: { open: vi.fn(async () => 'S1'), openLocal: vi.fn(async () => 'L1') },
    rdpSessions: { open: vi.fn(async () => 'R1') },
    splits: { open: vi.fn(async () => 'split-1'), openSaved: vi.fn(async () => 'split-1') },
  };
  return { bridge, ...deps, actions: createSidebarActions(deps) };
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('sidebar actions', () => {
  it('toasts when a folder cycle was refused', async () => {
    const { actions, toast, bridge } = setup({ move_items: { moved: 1, cycleRefused: 1 } });
    await actions.move(['f:f1', 'e:c1'], 'f2');
    expect(bridge.invoke).toHaveBeenCalledWith('move_items', {
      tags: ['f:f1', 'e:c1'],
      targetFolderId: 'f2',
    });
    expect(toast.current?.message).toBe("A folder can't be moved into itself");
  });

  it('shows errors in the alert', async () => {
    const { actions, alerts } = setup({
      move_items: () => {
        throw { code: 'readOnly', message: 'x', detail: null };
      },
    });
    await actions.move(['e:c1'], null);
    expect(alerts.current).toBe('The database is open read-only because another computer is using it.');
  });

  it('toasts the number of changed links', async () => {
    const { actions, toast } = setup({ set_credential: 3 });
    await actions.setCredential('k1', ['f:f1']);
    expect(toast.current?.message).toBe('Linked credential set — 3 connections');
    toast.clear();
    await actions.setCredential(null, ['e:c1']);
    expect(toast.current?.message).toBe('Linked credential removed — 3 connections');
  });

  it('confirms a bulk delete, then clears the selection and deletes', async () => {
    const { actions, dialogs, library, bridge } = setup({
      delete_items: { connections: 1, folders: 1, credentials: 0, splits: 0 },
    });
    library.setSelection(['e:c1', 'f:f3', 's:s1']);
    const done = actions.deleteItems(library.selection);
    expect(dialogs.current?.title).toBe('Delete 2 items?');
    expect(dialogs.current?.kind === 'confirm' && dialogs.current.destructive).toBe(true);
    dialogs.accept();
    await done;
    expect(library.selection).toEqual([]);
    expect(bridge.invoke).toHaveBeenCalledWith('delete_items', { tags: ['e:c1', 'f:f3'] });
  });

  it('a cancelled bulk delete deletes nothing', async () => {
    const { actions, dialogs, bridge } = setup();
    const done = actions.deleteItems(['e:c1']);
    dialogs.cancel();
    await done;
    expect(bridge.invoke).not.toHaveBeenCalled();
  });

  it('deletes an unused credential at once and asks for a used one', async () => {
    const { actions, dialogs, bridge } = setup({
      delete_items: { connections: 0, folders: 0, credentials: 1, splits: 0 },
    });
    await actions.deleteCredential('k2');
    expect(bridge.invoke).toHaveBeenLastCalledWith('delete_items', { tags: ['k:k2'] });
    const done = actions.deleteCredential('k1');
    expect(dialogs.current?.title).toBe('Delete credential?');
    expect(dialogs.current?.kind === 'confirm' && dialogs.current.message).toBe(
      '1 connection uses the credential “Domain admin”. After deleting it, it uses its own saved login.',
    );
    dialogs.accept();
    await done;
    expect(bridge.invoke).toHaveBeenLastCalledWith('delete_items', { tags: ['k:k1'] });
  });

  it('copies the password or says there is none', async () => {
    let has = true;
    const { actions, toast } = setup({
      copy_password: () => {
        if (!has) throw { code: 'noPassword', message: 'No saved password', detail: null };
      },
    });
    await actions.copyPassword('c1');
    expect(toast.current?.message).toBe('Password on the clipboard (30 s)');
    toast.clear();
    has = false;
    await actions.copyPassword('c1');
    expect(toast.current?.message).toBe('No saved password');
  });

  it('duplicates the entry with the localised suffix', async () => {
    const { actions, bridge } = setup({ duplicate_entry: 'new' });
    await actions.duplicate('e1');
    expect(bridge.invoke).toHaveBeenCalledWith('duplicate_entry', { id: 'e1', copySuffix: ' (copy)' });
  });

  it('stars the entry of a connection', async () => {
    const { actions, bridge } = setup({ set_favorite: null });
    await actions.setFavorite('h1', true);
    expect(bridge.invoke).toHaveBeenCalledWith('set_favorite', { tag: 'c:h1', isFavorite: true });
  });

  it('primary_action_on_an_entry_connects_its_default', async () => {
    const data = multiEntryLibrary();
    data.entries = data.entries.map((e) => (e.id === 'e1' ? { ...e, defaultConnectionId: 'h2' } : e));
    const { actions, sessions, rdpSessions } = setup({}, data);
    actions.primaryAction('e:e1');
    expect(rdpSessions.open).toHaveBeenCalledWith(expect.objectContaining({ id: 'h2' }));
    actions.primaryAction('c:h1');
    expect(sessions.open).toHaveBeenCalledWith(expect.objectContaining({ id: 'h1' }));
  });

  it('expands entries like folders, in the same setting', async () => {
    const { actions, bridge } = setup({}, multiEntryLibrary());
    await actions.setExpanded('e1');
    expect(bridge.invoke).toHaveBeenCalledWith('settings_set', { patch: { expandedFolders: ['e1'] } });
  });

  it('new items go into the folder of the selected entry or child connection', () => {
    const { actions, library } = setup();
    library.select('e:c3');
    expect(actions.selectedFolderId()).toBe('f2');
    library.select('fav:c2');
    expect(actions.selectedFolderId()).toBe('f1');
  });

  it('saves a split under the prompted name', async () => {
    const { actions, dialogs, toast, bridge, library } = setup({ save_saved_split: 's9' });
    const done = actions.saveAsSplit(
      library.library.connections.filter((c) => c.id === 'c1' || c.id === 'c3'),
    );
    expect(dialogs.current?.kind === 'prompt' && dialogs.current.initial).toBe('box + db');
    dialogs.accept('  Night shift ');
    await done;
    expect(bridge.invoke).toHaveBeenCalledWith('save_saved_split', {
      id: null,
      name: 'Night shift',
      connectionIds: ['c1', 'c3'],
    });
    expect(toast.current?.message).toBe('Saved split: Night shift');
  });

  it('renames a saved split; blank or cancelled names change nothing', async () => {
    const { actions, dialogs, bridge } = setup({ rename_saved_split: null });
    let done = actions.renameSavedSplit('s1');
    dialogs.accept('   ');
    await done;
    expect(bridge.invoke).not.toHaveBeenCalled();
    done = actions.renameSavedSplit('s1');
    dialogs.accept('Trio');
    await done;
    expect(bridge.invoke).toHaveBeenCalledWith('rename_saved_split', { id: 's1', name: 'Trio' });
  });

  it('opens saved splits and split views through the splits store', () => {
    const { actions, splits, library } = setup();
    actions.openSavedSplit('s1');
    expect(splits.openSaved).toHaveBeenCalledWith('s1');
    const c1 = library.connection('c1')!;
    actions.openSplit([c1, c1]);
    expect(splits.open).toHaveBeenCalledWith([c1, c1]);
    actions.openSplit([]);
    expect(splits.open).toHaveBeenCalledOnce();
  });

  it('runs the primary action per row kind', () => {
    const { actions, editor, rdpSessions, bridge } = setup();
    actions.primaryAction('k:k1');
    expect(editor.credential).toEqual({ mode: 'edit', id: 'k1' });
    actions.primaryAction('fav:c2');
    expect(rdpSessions.open).toHaveBeenCalledWith(expect.objectContaining({ id: 'c2' }));
    actions.primaryAction('f:f1');
    expect(bridge.invoke).toHaveBeenCalledWith('settings_set', { patch: { expandedFolders: ['f1'] } });
  });

  it('routes Connect: embedded session, external terminal or WinBox', async () => {
    const { actions, toast, sessions, rdpSessions, library, bridge } = setup({
      open_external_terminal: 'ghostty',
      connect_winbox: { kind: 'launched' },
    });
    const ssh = library.connection('c1')!;
    await actions.connect(ssh);
    expect(sessions.open).toHaveBeenCalledWith(ssh);

    await actions.connect({ ...ssh, sshTerminal: 'external' });
    expect(bridge.invoke).toHaveBeenCalledWith('open_external_terminal', { connectionId: 'c1' });
    expect(toast.current?.message).toBe('Connecting: box');
    toast.clear();

    // Menu overrides: the terminal and the MikroTik mode.
    await actions.connect(ssh, { terminal: 'external' });
    expect(bridge.invoke).toHaveBeenCalledTimes(2);
    toast.clear();
    const router = library.connection('c4')!;
    await actions.connect(router);
    expect(bridge.invoke).toHaveBeenCalledWith('connect_winbox', { connectionId: 'c4' });
    expect(toast.current?.message).toBe('Connecting: router');
    await actions.connect(router, { mode: 'ssh' });
    expect(sessions.open).toHaveBeenLastCalledWith(router);
    await actions.connect({ ...router, mikrotikMode: 'ssh' });
    expect(sessions.open).toHaveBeenCalledTimes(3);
    toast.clear();
    await actions.connect(library.connection('c2')!);
    expect(rdpSessions.open).toHaveBeenCalledTimes(1);
    await actions.connect(undefined);
    expect(sessions.open).toHaveBeenCalledTimes(3);
  });

  it('opens web connections in the browser and toasts the clipboard sequence', async () => {
    let outcome: LaunchOutcomeDto = { kind: 'loginCopied' };
    const { actions, toast, library, bridge } = setup({ connect_web: () => outcome });
    const portal = library.connection('c5')!;
    await actions.connect(portal);
    expect(bridge.invoke).toHaveBeenCalledWith('connect_web', { connectionId: 'c5' });
    expect(toast.current?.message).toBe('Opened: portal · password, then username on the clipboard');
    outcome = { kind: 'launched' };
    toast.clear();
    actions.primaryAction('e:c5');
    await flush();
    expect(toast.current?.message).toBe('Connecting: portal');
  });

  it('hands RDP set to the system client over and toasts the clipboard fallback', async () => {
    const { actions, toast, library, bridge } = setup({
      connect_rdp_external: { kind: 'passwordCopied', reason: 'Accessibility permission missing' },
    });
    const rdp = library.connection('c2')!;
    await actions.connect({ ...rdp, rdpClient: 'windowsApp' });
    expect(bridge.invoke).toHaveBeenCalledWith('connect_rdp_external', { connectionId: 'c2' });
    expect(toast.current?.message).toBe('Password on the clipboard (Accessibility permission missing)');
    toast.clear();
    await actions.connect(rdp, { rdpClient: 'windowsApp' });
    expect(bridge.invoke).toHaveBeenCalledTimes(2);
  });

  it('opens RDP set to the embedded client in a tab', async () => {
    const { actions, library, rdpSessions, bridge } = setup();
    const rdp = library.connection('c2')!;
    await actions.connect(rdp);
    expect(rdpSessions.open).toHaveBeenCalledWith(rdp);
    await actions.connect({ ...rdp, rdpClient: 'windowsApp' }, { rdpClient: 'embedded' });
    expect(rdpSessions.open).toHaveBeenCalledTimes(2);
    expect(bridge.invoke).not.toHaveBeenCalled();
  });

  it('shows a failed connect in the alert', async () => {
    const { actions, alerts, sessions, library } = setup({
      open_external_terminal: () => {
        throw { code: 'terminalFailed', message: "Couldn't open the terminal.", detail: 'boom' };
      },
    });
    sessions.open.mockRejectedValueOnce({
      code: 'sshNotFound',
      message: 'The ssh program is not installed.',
    });
    await actions.connect(library.connection('c1')!);
    expect(alerts.current).toBe('The ssh program is not installed.');
    alerts.dismiss();
    await actions.connect({ ...library.connection('c1')!, sshTerminal: 'external' });
    expect(alerts.current).toContain("Couldn't open the terminal.");
  });

  it('opens a local terminal and shows a failure in the alert', async () => {
    const { actions, alerts, sessions } = setup();
    await actions.openLocalTerminal();
    expect(sessions.openLocal).toHaveBeenCalledOnce();
    sessions.openLocal.mockRejectedValueOnce({
      code: 'spawnFailed',
      message: "Couldn't start the shell.",
      detail: 'boom',
    });
    await actions.openLocalTerminal();
    expect(alerts.current).toContain("Couldn't start the shell.");
  });

  it('keeps the expansion until Rust confirms it', async () => {
    const { actions } = setup();
    const done = actions.setExpanded('f2');
    expect([...actions.expanded]).toEqual(['f2']);
    await done;
    await flush();
    expect([...actions.expanded]).toEqual(['f2']);
    await actions.setExpanded('f1', true);
    expect([...actions.expanded]).toEqual(['f1', 'f2']);
  });

  it('opens editors in the selected folder', () => {
    const { actions, editor, library } = setup();
    library.select('e:c3');
    actions.newConnection('rdp');
    expect(editor.connection).toEqual({ mode: 'new', type: 'rdp', folderId: 'f2' });
    library.select('f:f1');
    actions.newFolder();
    expect(editor.folder).toEqual({ mode: 'new', parentId: 'f1' });
    library.select('k:k1');
    actions.newFolder();
    expect(editor.folder).toEqual({ mode: 'new', parentId: null });
    actions.editConnection('c1');
    expect(editor.connection).toEqual({ mode: 'edit', id: 'c1' });
    actions.renameFolder('f1');
    expect(editor.folder).toEqual({ mode: 'edit', id: 'f1' });
  });

  it('sets a folder colour keeping its name', async () => {
    const { actions, bridge } = setup({ save_folder: 'f1' });
    await actions.setFolderColor('f1', 'green');
    expect(bridge.invoke).toHaveBeenCalledWith('save_folder', {
      folder: { id: 'f1', name: 'Office', color: 'green' },
    });
  });

  it('sets up SSH key login with a busy state and result toasts', async () => {
    let finish: (value: unknown) => void = () => {};
    const { actions, toast, library, bridge } = setup({
      ssh_key_setup: () => new Promise((resolve) => (finish = resolve)),
    });
    const box = library.connection('c1')!;
    const running = actions.setupSshKey(box);
    expect(actions.settingUpKey('c1')).toBe(true);
    expect(toast.current?.message).toBe('Setting up SSH key login on c1.example.test…');
    // A second click while it runs does nothing.
    await actions.setupSshKey(box);
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_key_setup', { connectionId: 'c1' });
    finish({ host: 'c1.example.test', keyPath: '~/.ssh/k', generated: true, verified: true });
    await running;
    expect(actions.settingUpKey('c1')).toBe(false);
    expect(toast.current?.message).toBe('SSH key installed on c1.example.test');
  });

  it('keeps a failed login with the key and errors up as alerts', async () => {
    const { actions, alerts, library, bridge } = setup({
      ssh_key_setup: { host: 'h', keyPath: '~/.ssh/k', generated: false, verified: false },
    });
    const box = library.connection('c1')!;
    await actions.setupSshKey(box);
    expect(alerts.current).toBe('SSH key copied to h, but logging in with it failed');
    alerts.dismiss();
    bridge.replies.ssh_key_setup = () => {
      throw { code: 'keyInstallFailed', message: "Couldn't install the SSH key: %@", detail: 'denied' };
    };
    await actions.setupSshKey(box);
    expect(alerts.current).toBe("Couldn't install the SSH key: denied");
    expect(actions.settingUpKey('c1')).toBe(false);
  });
});
