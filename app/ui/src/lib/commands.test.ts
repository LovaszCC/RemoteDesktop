import { describe, expect, it, vi } from 'vitest';
import { createCommandRunner, filesTarget } from './commands.svelte';
import { TabsStore } from './tabs.svelte';
import { libraryOf } from './testing/fixtures';

function setup(
  state: {
    unlocked?: boolean;
    readOnly?: boolean;
    primary?: string | null;
    selection?: string[];
    session?: { id: string; connectionId?: string; status: { kind: 'running' | 'exited' } };
    rdp?: { id: string; status: { kind: 'connecting' | 'connected' | 'disconnected' } };
  } = {},
) {
  const deps = {
    app: {
      isUnlocked: state.unlocked ?? true,
      readOnly: state.readOnly ?? false,
      lock: vi.fn(async () => ({}) as never),
    },
    library: {
      primary: state.primary ?? null,
      selection: state.selection ?? (state.primary ? [state.primary] : []),
      library: libraryOf(['1', '2', '3']),
    },
    tabs: new TabsStore(),
    actions: {
      newConnection: vi.fn(),
      newFolder: vi.fn(),
      newCredential: vi.fn(),
      connect: vi.fn(),
      primaryAction: vi.fn(),
      openSplit: vi.fn(),
      openFiles: vi.fn(async () => undefined),
      openLocalTerminal: vi.fn(async () => {}),
    },
    alerts: { show: vi.fn() },
    settingsWindow: { open: vi.fn() },
    importer: { start: vi.fn(async () => {}) },
    exporter: { open: vi.fn() },
    transferImporter: { start: vi.fn(async () => {}) },
    sessions: {
      active: state.session as never,
      clear: vi.fn(async () => {}),
      toggleRecording: vi.fn(async () => {}),
      reconnect: vi.fn(async () => {}),
    },
    splits: { splitActive: vi.fn(async () => {}), toggleBroadcastActive: vi.fn() },
    rdpSessions: {
      active: state.rdp as never,
      reconnect: vi.fn(async () => {}),
      typeClipboard: vi.fn(async () => {}),
    },
    updates: { checkManually: vi.fn(async () => {}) },
    sidebarLayout: { toggle: vi.fn() },
    dev: true,
  };
  return { deps, run: createCommandRunner(deps) };
}

describe('command runner', () => {
  it('toggles the sidebar', () => {
    const { deps, run } = setup();
    run('toggleSidebar');
    expect(deps.sidebarLayout.toggle).toHaveBeenCalledOnce();
  });

  it('runs the terminal commands on the active session', () => {
    const running = setup({ session: { id: 'S1', status: { kind: 'running' } } });
    running.run('clearTerminal');
    running.run('recordOutput');
    running.run('reconnect');
    expect(running.deps.sessions.clear).toHaveBeenCalledWith('S1');
    expect(running.deps.sessions.toggleRecording).toHaveBeenCalledWith('S1');
    expect(running.deps.sessions.reconnect).not.toHaveBeenCalled();

    const ended = setup({ session: { id: 'S1', status: { kind: 'exited' } } });
    ended.run('recordOutput');
    ended.run('reconnect');
    expect(ended.deps.sessions.toggleRecording).not.toHaveBeenCalled();
    expect(ended.deps.sessions.reconnect).toHaveBeenCalledWith('S1');

    const none = setup();
    none.run('clearTerminal');
    none.run('reconnect');
    expect(none.deps.sessions.clear).not.toHaveBeenCalled();
  });

  it('types the clipboard into and reconnects the active RDP session', () => {
    const connected = setup({ rdp: { id: 'R1', status: { kind: 'connected' } } });
    connected.run('typeClipboard');
    connected.run('reconnect');
    expect(connected.deps.rdpSessions.typeClipboard).toHaveBeenCalledWith('R1');
    expect(connected.deps.rdpSessions.reconnect).not.toHaveBeenCalled();

    const ended = setup({ rdp: { id: 'R1', status: { kind: 'disconnected' } } });
    ended.run('typeClipboard');
    ended.run('reconnect');
    expect(ended.deps.rdpSessions.typeClipboard).not.toHaveBeenCalled();
    expect(ended.deps.rdpSessions.reconnect).toHaveBeenCalledWith('R1');

    const none = setup();
    none.run('typeClipboard');
    expect(none.deps.rdpSessions.typeClipboard).not.toHaveBeenCalled();
  });

  it('opens the editors when unlocked and writable', () => {
    const { deps, run } = setup();
    run('newRDP');
    run('newMikrotik');
    run('newFolder');
    run('newCredential');
    expect(deps.actions.newConnection.mock.calls).toEqual([['rdp'], ['mikrotik']]);
    expect(deps.actions.newFolder).toHaveBeenCalled();
    expect(deps.actions.newCredential).toHaveBeenCalled();
  });

  it('does nothing that needs the library while locked or read-only', () => {
    for (const state of [{ unlocked: false }, { readOnly: true }]) {
      const { deps, run } = setup({ ...state, primary: 'c:1' });
      run('newSSH');
      run('newFolder');
      run('importRDM');
      run('importTransfer');
      expect(deps.transferImporter.start).not.toHaveBeenCalled();
      expect(deps.actions.newConnection).not.toHaveBeenCalled();
      expect(deps.actions.newFolder).not.toHaveBeenCalled();
      expect(deps.importer.start).not.toHaveBeenCalled();
    }
    const locked = setup({ unlocked: false, primary: 'c:1' });
    locked.run('connect');
    locked.run('lock');
    expect(locked.deps.actions.primaryAction).not.toHaveBeenCalled();
    expect(locked.deps.app.lock).not.toHaveBeenCalled();
  });

  it('connects the primary selection when it is a connection or a split', () => {
    const { deps, run } = setup({ primary: 'c:1' });
    run('connect');
    expect(deps.actions.primaryAction).toHaveBeenCalledWith('c:1');
    const entry = setup({ primary: 'e:2' });
    entry.run('connect');
    expect(entry.deps.actions.primaryAction).toHaveBeenCalledWith('e:2');
    const folder = setup({ primary: 'f:1' });
    folder.run('connect');
    expect(folder.deps.actions.primaryAction).not.toHaveBeenCalled();
  });

  it('connects a multi-selection as a split view of its SSH connections', () => {
    const { deps, run } = setup({ primary: 'c:1', selection: ['c:1', 'f:x', 'fav:3'] });
    run('connect');
    expect(deps.actions.primaryAction).not.toHaveBeenCalled();
    expect(deps.actions.openSplit.mock.calls[0]?.[0].map((c: { id: string }) => c.id)).toEqual(['1', '3']);
    const one = setup({ primary: 'c:1', selection: ['c:1', 'f:x'] });
    one.run('connect');
    expect(one.deps.actions.openSplit).not.toHaveBeenCalled();
    expect(one.deps.actions.primaryAction).not.toHaveBeenCalled();
  });

  it('splits the active tab and toggles broadcast in the active split', () => {
    const { deps, run } = setup();
    run('splitTerminal');
    run('broadcastAll');
    expect(deps.splits.splitActive).toHaveBeenCalledOnce();
    expect(deps.splits.toggleBroadcastActive).toHaveBeenCalledOnce();
  });

  it('starts the imports, locks and opens Settings', () => {
    const { deps, run } = setup();
    run('importWinbox');
    run('importRDM');
    expect(deps.importer.start.mock.calls).toEqual([['winbox'], ['rdm']]);
    run('importTransfer');
    expect(deps.transferImporter.start).toHaveBeenCalledOnce();
    run('exportLibrary');
    expect(deps.exporter.open).toHaveBeenCalledWith(deps.library.library, deps.library.selection);
    run('lock');
    expect(deps.app.lock).toHaveBeenCalled();
    run('openSettings');
    expect(deps.settingsWindow.open).toHaveBeenCalled();
    run('checkForUpdates');
    expect(deps.updates.checkManually).toHaveBeenCalled();
  });

  it('drives the tabs', () => {
    const { deps, run } = setup();
    run('nextTab');
    expect(deps.tabs.active).toBeNull();
    run('devOpenTestTab');
    run('devOpenTestTab');
    const [a, b] = deps.tabs.ids;
    expect(deps.tabs.active).toBe(b);
    run('nextTab');
    expect(deps.tabs.active).toBeNull();
    run('previousTab');
    expect(deps.tabs.active).toBe(b);
    run('closeTab');
    expect(deps.tabs.ids).toEqual([a]);
  });

  it('shows the alert when locking fails', async () => {
    const { deps, run } = setup();
    deps.app.lock.mockRejectedValueOnce({ code: 'internal', message: 'The command failed.', detail: null });
    run('lock');
    await vi.waitFor(() => expect(deps.alerts.show).toHaveBeenCalled());
  });

  it('opens the file browser for the active terminal or one selected SSH connection', () => {
    const terminal = setup({ session: { id: 'S1', connectionId: '2', status: { kind: 'running' } } });
    terminal.run('openFiles');
    expect(terminal.deps.actions.openFiles).toHaveBeenCalledWith('2', { sessionId: 'S1' });

    const selected = setup({ primary: 'c:1' });
    selected.run('openFiles');
    expect(selected.deps.actions.openFiles).toHaveBeenCalledWith('1', { sessionId: undefined });
    expect(filesTarget(selected.deps)).toEqual({ connectionId: '1' });
    expect(filesTarget(setup({ primary: 'e:2' }).deps)).toEqual({ connectionId: '2' });
    expect(filesTarget(setup({ primary: 'fav:3' }).deps)).toEqual({ connectionId: '3' });

    const rdp = setup({ primary: 'c:3' });
    rdp.deps.library.library.connections[2]!.type = 'rdp';
    rdp.run('openFiles');
    expect(rdp.deps.actions.openFiles).not.toHaveBeenCalled();

    const several = setup({ primary: 'c:1', selection: ['c:1', 'c:2'] });
    several.run('openFiles');
    const locked = setup({ primary: 'c:1', unlocked: false });
    locked.run('openFiles');
    const folder = setup({ primary: 'f:x' });
    folder.run('openFiles');
    expect(several.deps.actions.openFiles).not.toHaveBeenCalled();
    expect(locked.deps.actions.openFiles).not.toHaveBeenCalled();
    expect(folder.deps.actions.openFiles).not.toHaveBeenCalled();

    const local = setup({
      primary: 'c:1',
      session: { id: 'S1', connectionId: 'local-terminal', status: { kind: 'running' } },
    });
    local.run('openFiles');
    expect(local.deps.actions.openFiles).not.toHaveBeenCalled();
    expect(filesTarget(local.deps)).toBeNull();
  });

  it('opens a local terminal while unlocked', () => {
    const { deps, run } = setup();
    run('newLocalTerminal');
    expect(deps.actions.openLocalTerminal).toHaveBeenCalledOnce();
    const locked = setup({ unlocked: false });
    locked.run('newLocalTerminal');
    expect(locked.deps.actions.openLocalTerminal).not.toHaveBeenCalled();
  });
});
