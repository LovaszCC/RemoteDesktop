import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import NeedsUpdateView from './NeedsUpdateView.svelte';

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

const settle = async () => {
  for (let i = 0; i < 3; i++) await new Promise((r) => setTimeout(r, 0));
  flushSync();
};

function render(reopen: () => Promise<unknown> = async () => ({})) {
  const deps = {
    api: { appQuit: vi.fn(async () => {}) },
    app: { reopenDatabase: vi.fn(reopen) },
    updates: { checkManually: vi.fn(async () => {}) },
    alerts: { show: vi.fn() },
  };
  mounted = mount(NeedsUpdateView, {
    target: document.body,
    props: {
      phase: { kind: 'needsUpdate', databaseVersion: '1.7.0', appVersion: '1.6.6' },
      ...deps,
    } as never,
  });
  flushSync();
  return deps;
}

const button = (label: string) =>
  [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label)!;

describe('NeedsUpdateView', () => {
  it('names both versions and opens the update dialog at once', () => {
    const deps = render();
    expect(document.body.textContent).toContain('Update required');
    expect(document.body.textContent).toContain('RemoteDesktop 1.7.0');
    expect(document.body.textContent).toContain('version 1.6.6');
    expect(deps.updates.checkManually).toHaveBeenCalledTimes(1);
  });

  it('checks again, retries the open and quits', async () => {
    const deps = render();
    button('Check for updates').click();
    expect(deps.updates.checkManually).toHaveBeenCalledTimes(2);
    button('Retry').click();
    await settle();
    expect(deps.app.reopenDatabase).toHaveBeenCalled();
    button('Quit').click();
    expect(deps.api.appQuit).toHaveBeenCalled();
  });

  it('shows a failed retry', async () => {
    const deps = render(async () => {
      throw { code: 'openFailed', message: 'nope' };
    });
    button('Retry').click();
    await settle();
    expect(deps.alerts.show).toHaveBeenCalled();
  });
});
