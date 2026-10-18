import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { setLocale } from '../i18n/index.svelte';
import { fakeBridge } from '../testing/bridge';
import type { UpdateCheckDto } from '../types';
import { notesText, progressFraction, progressText, updateStatusText } from './model';
import UpdateDialog from './UpdateDialog.svelte';
import { createUpdateStore, type AvailableUpdate } from './updates.svelte';

const AVAILABLE: AvailableUpdate = {
  kind: 'available',
  currentVersion: '0.1.0',
  version: '0.2.0',
  notes: "## What's Changed\n* Fix launch crash",
  date: '2026-10-01T10:00:00Z',
};

const UP_TO_DATE: UpdateCheckDto = { kind: 'upToDate', currentVersion: '0.1.0' };

const settle = async () => {
  for (let i = 0; i < 3; i++) await new Promise((r) => setTimeout(r, 0));
  flushSync();
};

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
  setLocale('en');
});

const failure = (code: string, message: string) => ({ code, message, detail: 'offline' });

describe('update store', () => {
  it('reports every manual check and opens the dialog', async () => {
    let reply: UpdateCheckDto = UP_TO_DATE;
    const bridge = fakeBridge({ update_check: () => reply });
    const store = createUpdateStore(bridge);
    expect(store.phase.kind).toBe('idle');
    await store.checkManually();
    expect(bridge.invoke).toHaveBeenCalledWith('update_check', { automatic: false });
    expect(store.isOpen).toBe(true);
    expect(store.phase).toEqual(UP_TO_DATE);

    reply = { kind: 'notConfigured', currentVersion: '0.1.0' };
    await store.checkManually();
    expect(store.phase.kind).toBe('notConfigured');

    reply = AVAILABLE;
    await store.checkManually();
    expect(store.phase).toEqual({ kind: 'available', update: AVAILABLE });
    store.close();
    expect(store.isOpen).toBe(false);
  });

  it('turns a failed check into a translated message', async () => {
    const bridge = fakeBridge({
      update_check: () => {
        throw failure('updateCheckFailed', "Couldn't check for updates.");
      },
    });
    const store = createUpdateStore(bridge);
    await store.checkManually();
    expect(store.phase).toEqual({
      kind: 'failed',
      message: "Couldn't check for updates. (offline)",
      update: null,
    });
  });

  it('keeps the launch check silent unless there is a newer version', async () => {
    let reply: UpdateCheckDto | Error = { kind: 'skipped' };
    const bridge = fakeBridge({
      update_check: () => {
        if (reply instanceof Error) throw reply;
        return reply;
      },
    });
    const store = createUpdateStore(bridge);
    await store.checkAutomatically();
    expect(bridge.invoke).toHaveBeenCalledWith('update_check', { automatic: true });
    for (const next of [UP_TO_DATE, { kind: 'notConfigured', currentVersion: '0.1.0' } as const]) {
      reply = next;
      await store.checkAutomatically();
    }
    reply = new Error('offline');
    await store.checkAutomatically();
    expect(store.phase.kind).toBe('idle');
    expect(store.isOpen).toBe(false);

    reply = AVAILABLE;
    await store.checkAutomatically();
    expect(store.phase.kind).toBe('available');
    expect(store.isOpen).toBe(true);
  });

  it('installs with progress, then relaunches', async () => {
    let finish: () => void = () => {};
    const bridge = fakeBridge({
      update_check: AVAILABLE,
      update_install: () => new Promise<void>((resolve) => (finish = resolve)),
      update_relaunch: undefined,
    });
    const store = createUpdateStore(bridge);
    await store.install();
    expect(bridge.invoke).not.toHaveBeenCalled();

    await store.checkManually();
    const installing = store.install();
    await settle();
    expect(store.phase).toMatchObject({ kind: 'installing', downloaded: 0, total: null });
    bridge.emit('update-progress', { downloaded: 512, total: 1024 });
    expect(store.phase).toMatchObject({ kind: 'installing', downloaded: 512, total: 1024 });
    finish();
    await installing;
    expect(store.phase).toEqual({ kind: 'installed', update: AVAILABLE });
    expect(bridge.listenerCount('update-progress')).toBe(0);

    await store.relaunch();
    expect(bridge.invoke).toHaveBeenLastCalledWith('update_relaunch', undefined);
  });

  it('lets a failed install be retried', async () => {
    let attempts = 0;
    const bridge = fakeBridge({
      update_check: AVAILABLE,
      update_install: () => {
        attempts += 1;
        if (attempts === 1) throw failure('updateInstallFailed', "The update couldn't be installed.");
      },
    });
    const store = createUpdateStore(bridge);
    await store.checkManually();
    await store.install();
    expect(store.phase).toMatchObject({ kind: 'failed', update: AVAILABLE });
    await store.install();
    expect(store.phase.kind).toBe('installed');
    expect(attempts).toBe(2);
  });
});

describe('update texts', () => {
  it('summarize the phase for Settings', () => {
    expect(updateStatusText({ kind: 'idle' })).toBeNull();
    expect(updateStatusText({ kind: 'checking' })).toBe('Checking for updates…');
    expect(updateStatusText({ kind: 'upToDate', currentVersion: '0.1.0' })).toBe(
      'RemoteDesktop is up to date.',
    );
    expect(updateStatusText({ kind: 'notConfigured', currentVersion: '0.1.0' })).toBe(
      "Updates aren't set up in this build.",
    );
    expect(updateStatusText({ kind: 'available', update: AVAILABLE })).toBe(
      'RemoteDesktop 0.2.0 is available.',
    );
    expect(updateStatusText({ kind: 'installed', update: AVAILABLE })).toBe('The update is installed.');
    expect(updateStatusText({ kind: 'failed', message: 'Nope', update: null })).toBe('Nope');
    setLocale('hu');
    expect(updateStatusText({ kind: 'notConfigured', currentVersion: '0.1.0' })).toBe(
      'Ebben a buildben nincs beállítva a frissítés.',
    );
  });

  it('drop heading marks from the notes and format the progress', () => {
    expect(notesText(AVAILABLE.notes)).toBe("What's Changed\n* Fix launch crash");
    expect(notesText(null)).toBe('');
    expect(progressText(1500, null, 'en')).toBe('1.5 kB');
    expect(progressText(1_000_000, 2_000_000, 'en')).toBe('1 MB of 2 MB');
    expect(progressFraction(5, 10)).toBe(0.5);
    expect(progressFraction(5, null)).toBeNull();
    expect(progressFraction(20, 10)).toBe(1);
  });
});

describe('update dialog', () => {
  async function render(replies: Parameters<typeof fakeBridge>[0]) {
    const bridge = fakeBridge(replies);
    const store = createUpdateStore(bridge);
    mounted = mount(UpdateDialog, { target: document.body, props: { store } });
    await settle();
    return { bridge, store };
  }

  const dialog = () => document.querySelector<HTMLElement>('[data-testid="update-dialog"]');
  const button = (label: string) =>
    [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);

  it('is hidden until a check opens it, and OK closes it', async () => {
    const { store } = await render({ update_check: UP_TO_DATE });
    expect(dialog()).toBeNull();
    await store.checkManually();
    await settle();
    expect(dialog()?.dataset.phase).toBe('upToDate');
    expect(dialog()?.textContent).toContain('You have version 0.1.0.');
    button('OK')!.click();
    await settle();
    expect(dialog()).toBeNull();
  });

  it('shows the notes and installs, then offers the restart', async () => {
    const { bridge, store } = await render({
      update_check: AVAILABLE,
      update_install: undefined,
      update_relaunch: undefined,
    });
    await store.checkManually();
    await settle();
    expect(dialog()?.textContent).toContain('RemoteDesktop 0.2.0 is available.');
    expect(document.querySelector('[data-testid="update-notes"]')?.textContent).toBe(
      "What's Changed\n* Fix launch crash",
    );
    button('Install update')!.click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('update_install', undefined);
    expect(dialog()?.dataset.phase).toBe('installed');
    button('Restart now')!.click();
    await settle();
    expect(bridge.invoke).toHaveBeenLastCalledWith('update_relaunch', undefined);
  });

  it('retries a failed check with Try again', async () => {
    let fail = true;
    const { store } = await render({
      update_check: () => {
        if (fail) throw failure('updateCheckFailed', "Couldn't check for updates.");
        return UP_TO_DATE;
      },
    });
    await store.checkManually();
    await settle();
    expect(dialog()?.textContent).toContain("Couldn't check for updates.");
    fail = false;
    button('Try again')!.click();
    await settle();
    expect(dialog()?.dataset.phase).toBe('upToDate');
  });
});
