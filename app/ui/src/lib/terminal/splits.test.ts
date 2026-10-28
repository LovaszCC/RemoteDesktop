import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createSettingsStore } from '../stores/settings.svelte';
import SplitView from './SplitView.svelte';
import { createApi } from '../api';
import { createAlertStore } from '../stores/alert.svelte';
import { createLibraryStore } from '../stores/library.svelte';
import { createToastStore } from '../stores/toast.svelte';
import { TabsStore } from '../tabs.svelte';
import { fakeBridge } from '../testing/bridge';
import { libraryOf } from '../testing/fixtures';
import type { LabelColor, SshEvent, SshEventDto } from '../types';
import { createSessionsStore, type OutputChannel } from './sessions.svelte';
import { createSplitsStore } from './splits.svelte';
import { LOCAL_TERMINAL } from './local';
import type { TerminalHandle, TerminalOptions } from './xterm';

function setup(
  replies: Parameters<typeof fakeBridge>[0] = {},
  colors: (LabelColor | null)[] = ['red', 'red'],
) {
  let n = 0;
  const bridge = fakeBridge({
    open_ssh_session: () => ({ sessionId: `S${++n}` }),
    ssh_input: null,
    ssh_broadcast_input: null,
    ssh_resize: null,
    ssh_close: null,
    ssh_reconnect: null,
    save_saved_split: 'saved-1',
    ...replies,
  });
  const library = createLibraryStore(bridge);
  const dto = libraryOf(['a', 'b', 'c']);
  dto.connections = dto.connections.map((c, i) => ({ ...c, color: colors[i] ?? null }));
  dto.savedSplits = [
    { id: 'sv', name: 'switches', connectionIds: ['b', 'gone', 'a', 'a'], sortOrder: 0, createdAt: '' },
    { id: 'empty', name: 'old', connectionIds: ['gone'], sortOrder: 1, createdAt: '' },
    { id: 'pair', name: 'pair', connectionIds: ['b', 'a'], sortOrder: 2, createdAt: '' },
  ];
  library.replace(dto);
  const tabs = new TabsStore();
  const toast = createToastStore();
  const alerts = createAlertStore();
  const terminals: { options: TerminalOptions; handle: TerminalHandle }[] = [];
  const api = createApi(bridge.invoke);
  const sessions = createSessionsStore({
    api,
    listen: bridge.listen,
    tabs,
    toast,
    alerts,
    createTerminal: (options) => {
      const handle = {
        write: vi.fn(),
        clearLocal: vi.fn(),
        attach: vi.fn(),
        detach: vi.fn(),
        fit: vi.fn(),
        size: vi.fn(() => ({ cols: 80, rows: 24 })),
        focus: vi.fn(),
        dispose: vi.fn(),
      };
      terminals.push({ options, handle });
      return handle;
    },
    createChannel: () => ({ onmessage: () => {} }) as OutputChannel,
    platform: () => 'linux',
    shortcuts: () => [],
  });
  const prompt = vi.fn(async (): Promise<string | null> => 'my split');
  let ids = 0;
  const splits = createSplitsStore({
    api,
    sessions,
    tabs,
    library,
    dialogs: { prompt },
    toast,
    alerts,
    newId: () => `T${++ids}`,
  });
  const input = (index: number, data: string) => terminals[index]!.options.onInput(data);
  return { bridge, library, tabs, toast, alerts, sessions, splits, terminals, prompt, input };
}

const event = (sessionId: string, e: SshEvent) => ({ sessionId, ...e }) as SshEventDto;
const exit = (code: number) => ({ kind: 'exit', code, authFailed: false }) as const;
const panes = (splits: ReturnType<typeof setup>['splits'], id: string) =>
  splits.get(id)!.panes.map((p) => p.id);

describe('splits store', () => {
  it('opens connections as panes of one tab with the common colour', async () => {
    const { splits, tabs, library, sessions } = setup();
    const id = await splits.open([library.connection('a')!, library.connection('b')!]);
    expect(id).toBe('T1');
    expect(panes(splits, 'T1')).toEqual(['S1', 'S2']);
    expect(tabs.ids).toEqual(['T1']);
    expect(tabs.active).toBe('T1');
    expect(tabs.session('T1')).toMatchObject({ title: 'a + b', type: 'ssh', color: 'red', split: true });
    expect(sessions.isPane('S1')).toBe(true);
    expect(sessions.runningCount).toBe(2);
    // On a split tab the session commands act on the focused pane.
    expect(sessions.active?.id).toBe('S1');
    splits.focus('S2');
    expect(sessions.active?.id).toBe('S2');
  });

  it('opens at most four panes with a toast, and no tab when none starts', async () => {
    const { splits, library, toast, tabs, alerts } = setup();
    const a = library.connection('a')!;
    await splits.open([a, a, a, a, a]);
    expect(panes(splits, 'T1')).toHaveLength(4);
    expect(toast.current?.message).toBe('A split view holds at most 4 terminals.');
    expect(tabs.session('T1')?.title).toBe('Split (4)');

    const failing = setup({
      open_ssh_session: () => {
        throw { code: 'sshNotFound', message: 'ssh is missing', detail: null };
      },
    });
    expect(await failing.splits.open([a])).toBeNull();
    expect(failing.tabs.ids).toEqual([]);
    expect(failing.alerts.current).toBeTruthy();
    expect(alerts.current).toBeNull();
    expect(await splits.open([])).toBeNull();
  });

  it('splits a local terminal with another shell and leaves local panes out of saved splits', async () => {
    let n = 0;
    const { splits, sessions, tabs, library, toast, bridge, prompt } = setup({
      open_local_terminal: () => ({ sessionId: `L${++n}` }),
    });
    await sessions.openLocal();
    await splits.splitActive();
    expect(panes(splits, 'T1')).toEqual(['L1', 'L2']);
    expect(splits.get('T1')!.panes.map((p) => p.connectionId)).toEqual(['local-terminal', 'local-terminal']);
    expect(tabs.session('T1')?.title).toBe('Local Terminal + Local Terminal 2');
    splits.focus('L2');
    await splits.splitActive();
    expect(panes(splits, 'T1')).toEqual(['L1', 'L2', 'L3']);

    await splits.save('T1');
    expect(prompt).not.toHaveBeenCalled();
    expect(toast.current?.message).toBe("Local terminals can't be saved in a split.");

    await splits.addPane('T1', library.connection('a')!);
    await splits.save('T1');
    expect(bridge.invoke).toHaveBeenCalledWith('save_saved_split', {
      id: null,
      name: 'my split',
      connectionIds: ['a'],
    });
  });

  it('opens a saved split with its existing connections and broadcast off', async () => {
    const { splits, sessions, toast } = setup();
    await splits.openSaved('sv');
    const split = splits.get('T1')!;
    expect(split.panes.map((p) => p.connectionId)).toEqual(['b', 'a', 'a']);
    expect(split.broadcasting).toEqual([]);
    expect(split.savedSplitId).toBe('sv');
    expect(sessions.runningCount).toBe(3);
    expect(await splits.openSaved('empty')).toBeNull();
    expect(toast.current?.message).toBe("None of this split's connections exist any more.");
    expect(await splits.openSaved('unknown')).toBeNull();
  });

  it('mixes colours to none and follows title and root changes of the panes', async () => {
    const { splits, library, tabs, sessions } = setup({}, ['red', 'blue']);
    await splits.open([library.connection('a')!, library.connection('b')!]);
    expect(tabs.session('T1')?.color).toBeNull();
    sessions.handleEvent(event('S2', { kind: 'title', title: 'root@b: ~' }));
    sessions.handleEvent(event('S2', { kind: 'root', root: true }));
    expect(tabs.session('T1')).toMatchObject({ title: 'a + root@b: ~', root: true });
  });

  it('routes input of a broadcasting pane to the other broadcasting, running panes', async () => {
    const { splits, library, bridge, input, sessions } = setup();
    const a = library.connection('a')!;
    await splits.open([a, a, a]);
    input(0, 'ls\r');
    expect(bridge.invoke).toHaveBeenLastCalledWith('ssh_input', { sessionId: 'S1', data: 'ls\r' });

    splits.toggleBroadcast('S1');
    expect(splits.isBroadcasting('S1')).toBe(true);
    input(0, 'ls\r');
    // S1 broadcasts but nobody else does.
    expect(bridge.invoke).toHaveBeenLastCalledWith('ssh_input', { sessionId: 'S1', data: 'ls\r' });

    splits.toggleBroadcast('S3');
    input(0, 'x');
    expect(bridge.invoke).toHaveBeenLastCalledWith('ssh_broadcast_input', {
      sourceSessionId: 'S1',
      targetSessionIds: ['S3'],
      data: 'x',
    });
    // S2 doesn't broadcast: its input stays its own.
    input(1, 'y');
    expect(bridge.invoke).toHaveBeenLastCalledWith('ssh_input', { sessionId: 'S2', data: 'y' });

    splits.toggleBroadcastAll('T1');
    expect(splits.isAllBroadcasting('T1')).toBe(true);
    sessions.handleEvent(event('S3', exit(1)));
    input(1, 'z');
    expect(bridge.invoke).toHaveBeenLastCalledWith('ssh_broadcast_input', {
      sourceSessionId: 'S2',
      targetSessionIds: ['S1'],
      data: 'z',
    });
    splits.toggleBroadcastAll('T1');
    expect(splits.get('T1')!.broadcasting).toEqual([]);
  });

  it('turns the tab orange while a pane broadcasts and toggles all on the active split', async () => {
    const { splits, library, tabs } = setup();
    await splits.open([library.connection('a')!, library.connection('b')!]);
    splits.toggleBroadcast('S2');
    expect(tabs.session('T1')?.broadcasting).toBe(true);
    splits.toggleBroadcastActive();
    expect(splits.isAllBroadcasting('T1')).toBe(true);
    splits.toggleBroadcastActive();
    expect(tabs.session('T1')?.broadcasting).toBe(false);
    tabs.select(null);
    splits.toggleBroadcastActive();
    expect(splits.get('T1')!.broadcasting).toEqual([]);
  });

  it('closes a pane on exit 0, keeps it on other exits, and the tab with the last pane', async () => {
    const { splits, library, tabs, sessions, bridge } = setup();
    const a = library.connection('a')!;
    await splits.open([a, a]);
    sessions.handleEvent(event('S1', exit(0)));
    expect(panes(splits, 'T1')).toEqual(['S2']);
    expect(sessions.has('S1')).toBe(false);
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_close', { sessionId: 'S1' });
    sessions.handleEvent(event('S2', exit(1)));
    expect(panes(splits, 'T1')).toEqual(['S2']);
    expect(sessions.state('S2')?.status.kind).toBe('exited');
    sessions.close('S2');
    expect(splits.has('T1')).toBe(false);
    expect(tabs.ids).toEqual([]);
  });

  it('ends every pane when the tab closes', async () => {
    const { splits, library, tabs, sessions } = setup();
    const a = library.connection('a')!;
    await splits.open([a, a]);
    tabs.closeSession('T1');
    expect(splits.has('T1')).toBe(false);
    expect(sessions.runningCount).toBe(0);
  });

  it('adds terminals up to four', async () => {
    const { splits, library, sessions } = setup();
    const a = library.connection('a')!;
    await splits.open([a, a]);
    await splits.addPane('T1', library.connection('c')!);
    expect(splits.get('T1')!.focused).toBe('S3');
    await splits.addPane('T1', a);
    expect(splits.isFull('T1')).toBe(true);
    await splits.addPane('T1', a);
    expect(panes(splits, 'T1')).toEqual(['S1', 'S2', 'S3', 'S4']);
    expect(sessions.runningCount).toBe(4);
    await splits.addPane('nope', a);
    expect(sessions.runningCount).toBe(4);
  });

  it('splits a terminal tab keeping its session and terminal, then the focused pane again', async () => {
    const { splits, library, tabs, sessions, terminals, bridge } = setup();
    const a = library.connection('a')!;
    await sessions.open(a);
    tabs.rename('S1', 'mine');
    expect(splits.canSplitActive).toBe(true);
    await splits.splitActive();
    expect(tabs.ids).toEqual(['T1']);
    expect(tabs.active).toBe('T1');
    expect(tabs.displayTitle(tabs.session('T1')!)).toBe('mine');
    expect(panes(splits, 'T1')).toEqual(['S1', 'S2']);
    expect(sessions.handle('S1')).toBe(terminals[0]!.handle);
    expect(terminals[0]!.handle.dispose).not.toHaveBeenCalled();
    expect(bridge.invoke).not.toHaveBeenCalledWith('ssh_close', { sessionId: 'S1' });
    expect(bridge.invoke.mock.calls.filter(([c]) => c === 'open_ssh_session')).toHaveLength(2);

    splits.focus('S1');
    await splits.splitActive();
    await splits.splitActive();
    expect(splits.get('T1')!.panes.map((p) => p.connectionId)).toEqual(['a', 'a', 'a', 'a']);
    expect(splits.canSplitActive).toBe(false);
    await splits.splitActive();
    expect(panes(splits, 'T1')).toHaveLength(4);

    tabs.select(null);
    expect(splits.canSplitActive).toBe(false);
    await splits.splitActive();
  });

  it('lists single terminal tabs in tab bar order as merge candidates', async () => {
    let n = 0;
    const { splits, sessions, tabs, library } = setup({
      open_local_terminal: () => ({ sessionId: `L${++n}` }),
    });
    await sessions.open(library.connection('a')!);
    tabs.openSession({ id: 'rdp', title: 'desk', type: 'rdp', color: null });
    await sessions.openLocal();
    await splits.open([library.connection('b')!, library.connection('c')!]);
    expect(tabs.ids).toEqual(['S1', 'rdp', 'L1', 'T1']);
    expect(splits.mergeCandidates).toEqual(['S1', 'L1']);
  });

  it('merges open terminal tabs into one split, keeping their sessions', async () => {
    let n = 0;
    const { splits, sessions, tabs, library, terminals, bridge } = setup({
      open_local_terminal: () => ({ sessionId: `L${++n}` }),
    });
    const closed: string[] = [];
    tabs.onSessionClosed((id) => closed.push(id));
    tabs.openSession({ id: 'rdp', title: 'desk', type: 'rdp', color: null });
    await sessions.open(library.connection('a')!);
    await sessions.openLocal();
    await sessions.open(library.connection('b')!);
    tabs.select('L1');

    expect(splits.mergeTabs(['S2', 'rdp', 'S1', 'L1'])).toBe('T1');
    expect(tabs.ids).toEqual(['rdp', 'T1']);
    expect(tabs.active).toBe('T1');
    expect(panes(splits, 'T1')).toEqual(['S1', 'L1', 'S2']);
    expect(splits.get('T1')!.panes.map((p) => p.connectionId)).toEqual(['a', 'local-terminal', 'b']);
    expect(splits.get('T1')!.focused).toBe('L1');
    expect(tabs.session('T1')).toMatchObject({ split: true, title: 'Split (3)' });
    expect(['S1', 'L1', 'S2'].every((id) => sessions.isPane(id))).toBe(true);
    expect(sessions.handle('L1')).toBe(terminals[1]!.handle);
    for (const t of terminals) expect(t.handle.dispose).not.toHaveBeenCalled();
    expect(closed).toEqual([]);
    expect(bridge.invoke.mock.calls.some(([c]) => c === 'ssh_close')).toBe(false);
    expect(splits.mergeCandidates).toEqual([]);
  });

  it('merges at most four tabs, focuses the first without the active tab, and needs two', async () => {
    const { splits, sessions, tabs, library } = setup();
    for (const id of ['a', 'b', 'c', 'a', 'b']) await sessions.open(library.connection(id)!);
    tabs.select(null);
    expect(splits.mergeTabs(['S1'])).toBeNull();
    expect(splits.mergeTabs(['S1', 'nope'])).toBeNull();
    expect(tabs.ids).toHaveLength(5);

    // A connection's tabs sit together: a, a, b, b, c.
    expect(tabs.ids).toEqual(['S1', 'S4', 'S2', 'S5', 'S3']);
    expect(splits.mergeTabs(['S5', 'S4', 'S3', 'S2', 'S1'])).toBe('T1');
    expect(panes(splits, 'T1')).toEqual(['S1', 'S4', 'S2', 'S5']);
    expect(splits.get('T1')!.focused).toBe('S1');
    expect(tabs.ids).toEqual(['T1', 'S3']);
    expect(splits.mergeTabs(['T1', 'S3'])).toBeNull();
  });

  it('moves a pane to its own tab, live, right after the split', async () => {
    let n = 0;
    const { splits, sessions, tabs, library, terminals, bridge } = setup({
      open_local_terminal: () => ({ sessionId: `L${++n}` }),
    });
    const closed: string[] = [];
    tabs.onSessionClosed((id) => closed.push(id));
    await splits.open([library.connection('a')!, LOCAL_TERMINAL, library.connection('b')!]);
    tabs.openSession({ id: 'rdp', title: 'desk', type: 'rdp', color: null });
    splits.toggleBroadcast('L1');
    splits.toggleBroadcast('S2');

    splits.detachPane('L1');
    expect(tabs.ids).toEqual(['T1', 'L1', 'rdp']);
    expect(tabs.active).toBe('L1');
    expect(tabs.session('L1')).toMatchObject({ title: 'Local Terminal', type: 'ssh', color: null });
    expect(tabs.session('L1')?.group).toBeUndefined();
    expect(sessions.isPane('L1')).toBe(false);
    expect(panes(splits, 'T1')).toEqual(['S1', 'S2']);
    expect(splits.get('T1')!.broadcasting).toEqual(['S2']);
    expect(splits.mergeCandidates).toEqual(['L1']);
    expect(closed).toEqual([]);
    expect(terminals[1]!.handle.dispose).not.toHaveBeenCalled();
    expect(bridge.invoke.mock.calls.some(([c]) => c === 'ssh_close')).toBe(false);

    // Typing in the moved-out terminal goes to it alone.
    terminals[1]!.options.onInput('x');
    expect(bridge.invoke).toHaveBeenLastCalledWith('ssh_input', { sessionId: 'L1', data: 'x' });
    splits.detachPane('nope');
  });

  it("turns the last pane into a plain tab in the split's place", async () => {
    const { splits, sessions, tabs, library } = setup();
    await splits.open([library.connection('a')!, library.connection('b')!]);
    tabs.rename('T1', 'pair');
    tabs.setColor('blue', 'T1');
    tabs.openSession({ id: 'rdp', title: 'desk', type: 'rdp', color: null });

    splits.detachPane('S1');
    expect(splits.has('T1')).toBe(false);
    expect(tabs.ids).toEqual(['S2', 'S1', 'rdp']);
    expect(tabs.active).toBe('S1');
    expect(tabs.session('S2')).toMatchObject({ title: 'b', group: 'b', color: 'red' });
    expect(tabs.displayTitle(tabs.session('S2')!)).toBe('b');
    expect(tabs.displayColor(tabs.session('S2')!)).toBe('red');
    expect(tabs.session('S1')).toMatchObject({ title: 'a', group: 'a', color: 'red' });
    expect(sessions.isPane('S1') || sessions.isPane('S2')).toBe(false);
    expect(splits.mergeCandidates).toEqual(['S2', 'S1']);

    // A split down to one pane (others closed) becomes that pane's tab.
    expect(splits.mergeTabs(['S1', 'S2'])).toBe('T2');
    splits.closePane('S2');
    splits.detachPane('S1');
    expect(tabs.ids).toEqual(['S1', 'rdp']);
    expect(sessions.isPane('S1')).toBe(false);
  });

  it('saves the panes as a split and updates the saved split it came from', async () => {
    const { splits, library, bridge, prompt, toast } = setup();
    const a = library.connection('a')!;
    await splits.open([a, library.connection('b')!]);
    await splits.save('T1');
    expect(prompt).toHaveBeenCalledWith(expect.objectContaining({ initial: 'a + b' }));
    expect(bridge.invoke).toHaveBeenCalledWith('save_saved_split', {
      id: null,
      name: 'my split',
      connectionIds: ['a', 'b'],
    });
    expect(splits.get('T1')!.savedSplitId).toBe('saved-1');
    expect(toast.current?.message).toBe('Saved split: my split');
    prompt.mockResolvedValueOnce(null);
    await splits.save('T1');
    expect(bridge.invoke.mock.calls.filter(([c]) => c === 'save_saved_split')).toHaveLength(1);

    await splits.openSaved('pair');
    expect(splits.canUpdateSaved('T2')).toBe(false);
    await splits.addPane('T2', a);
    expect(splits.canUpdateSaved('T2')).toBe(true);
    toast.clear();
    await splits.updateSaved('T2');
    expect(bridge.invoke).toHaveBeenLastCalledWith('save_saved_split', {
      id: 'pair',
      name: 'pair',
      connectionIds: ['b', 'a', 'a'],
    });
    expect(toast.current?.message).toBe('Saved split updated: pair');
    expect(splits.canUpdateSaved('T1')).toBe(false);
  });

  it('shows save errors in the alert', async () => {
    const { splits, library, alerts } = setup({
      save_saved_split: () => {
        throw { code: 'readOnly', message: 'x', detail: null };
      },
    });
    await splits.open([library.connection('a')!, library.connection('b')!]);
    await splits.save('T1');
    expect(alerts.current).toBeTruthy();
  });

  it('follows connection colour changes', async () => {
    const { splits, library, tabs } = setup();
    await splits.open([library.connection('a')!, library.connection('b')!]);
    const dto = libraryOf(['a', 'b', 'c']);
    dto.connections = dto.connections.map((c) => ({ ...c, color: 'green' }));
    library.replace(dto);
    splits.syncConnections();
    expect(tabs.session('T1')?.color).toBe('green');
  });
});

describe('SplitView', () => {
  let mounted: ReturnType<typeof mount> | undefined;
  afterEach(() => {
    if (mounted) unmount(mounted);
    mounted = undefined;
    document.body.innerHTML = '';
  });
  globalThis.ResizeObserver ??= class {
    observe() {}
    disconnect() {}
    unobserve() {}
  } as unknown as typeof ResizeObserver;

  it('lays out the panes, toggles broadcast and closes panes', async () => {
    const { splits, sessions, library, bridge, terminals } = setup();
    const a = library.connection('a')!;
    await splits.open([a, a, library.connection('b')!]);
    const settings = createSettingsStore({ ...bridge, onLanguage: () => {} });
    mounted = mount(SplitView, {
      target: document.body,
      props: { splits, sessions, id: 'T1', active: true, library, settings },
    });
    flushSync();
    const columns = document.querySelectorAll('.column');
    expect(columns).toHaveLength(2);
    expect(columns[1]!.querySelectorAll('[data-pane]')).toHaveLength(2);
    expect(document.querySelector('.count')?.textContent).toBe('3/4');
    expect(terminals[0]!.handle.attach).toHaveBeenCalled();
    const pane = (id: string) => document.querySelector<HTMLElement>(`[data-pane="${id}"]`)!;
    expect(pane('S1').classList.contains('focused')).toBe(true);

    pane('S2').querySelector<HTMLButtonElement>('[aria-pressed]')!.click();
    flushSync();
    expect(pane('S2').classList.contains('broadcasting')).toBe(true);
    expect(pane('S2').textContent).toContain('BROADCAST');

    document.querySelector<HTMLButtonElement>('.broadcast-all')!.click();
    flushSync();
    expect(splits.isAllBroadcasting('T1')).toBe(true);
    expect(document.querySelector('.broadcast-all')?.getAttribute('aria-pressed')).toBe('true');

    pane('S2').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    flushSync();
    expect(splits.get('T1')!.focused).toBe('S2');

    sessions.handleEvent(event('S3', exit(2)));
    flushSync();
    expect(pane('S3').querySelector('[role="alert"]')).not.toBeNull();

    pane('S3').querySelector<HTMLButtonElement>('[aria-label="Close this terminal"]')!.click();
    flushSync();
    expect(document.querySelectorAll('[data-pane]')).toHaveLength(2);
    expect(document.querySelectorAll('.column')).toHaveLength(2);

    pane('S2').querySelector<HTMLButtonElement>('[aria-label="Move to its own tab"]')!.click();
    flushSync();
    expect(splits.has('T1')).toBe(false);
    expect(sessions.isPane('S1')).toBe(false);
    expect(sessions.isPane('S2')).toBe(false);
    expect(document.querySelector('[data-pane]')).toBeNull();
  });
});
