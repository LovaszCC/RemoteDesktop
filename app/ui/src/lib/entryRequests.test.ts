import { describe, expect, it, vi } from 'vitest';
import { installEntryRequests } from './entryRequests';
import { createLibraryStore } from './stores/library.svelte';
import { createToastStore } from './stores/toast.svelte';
import { fakeBridge } from './testing/bridge';
import type { EntryDto, LibraryDto } from './types';

const settle = async () => {
  for (let i = 0; i < 3; i++) await new Promise((r) => setTimeout(r, 0));
};

function libraryWith(...ids: string[]): LibraryDto {
  return {
    folders: [],
    entries: ids.map((id) => ({ id, name: id, connectionIds: [] }) as unknown as EntryDto),
    connections: [],
    credentials: [],
    portForwards: [],
    savedSplits: [],
  } as unknown as LibraryDto;
}

function setup(pending: (string | null)[], served: LibraryDto = libraryWith('A')) {
  const queue = [...pending];
  const bridge = fakeBridge({
    take_entry_request: () => queue.shift() ?? null,
    library: served,
  });
  const library = createLibraryStore(bridge);
  const open = vi.fn();
  const toast = createToastStore();
  const requests = installEntryRequests({ ...bridge, library, open, toast });
  return { bridge, library, open, toast, requests };
}

describe('entry requests', () => {
  it('opens the requested entry like a double-click', async () => {
    const { open, requests } = setup(['A']);
    await requests.check();
    expect(open).toHaveBeenCalledWith('e:A');
    requests.stop();
  });

  it('toasts an entry that is not in the open database', async () => {
    const { open, toast, requests } = setup(['GONE']);
    await requests.check();
    expect(open).not.toHaveBeenCalled();
    expect(toast.current?.message).toBe('Entry not found');
    requests.stop();
  });

  it('does nothing when nothing waits', async () => {
    const { bridge, open, requests } = setup([null]);
    await requests.check();
    expect(open).not.toHaveBeenCalled();
    expect(bridge.invoke).not.toHaveBeenCalledWith('library', undefined);
    requests.stop();
  });

  it('checks again on entry-requested and after the unlock', async () => {
    const { bridge, open, requests } = setup([null, 'A', 'A']);
    await settle();
    await requests.check();
    bridge.emit('entry-requested', null);
    await settle();
    expect(open).toHaveBeenCalledTimes(1);
    bridge.emit('library-changed', libraryWith('A'));
    await settle();
    expect(open).toHaveBeenCalledTimes(2);
    requests.stop();
    expect(bridge.listenerCount('entry-requested')).toBe(0);
    expect(bridge.listenerCount('library-changed')).toBe(0);
  });
});
