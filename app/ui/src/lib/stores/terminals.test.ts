import { describe, expect, it } from 'vitest';
import { fakeBridge } from '../testing/bridge';
import { createTerminalsStore, externalTerminalName, externalTerminalOptions } from './terminals.svelte';

const installed = [
  { id: 'ghostty', name: 'Ghostty' },
  { id: 'terminal', name: 'Terminal' },
];

describe('external terminals', () => {
  it('offers the default, the installed ones and a saved one that is missing', () => {
    expect(externalTerminalOptions(null, installed)).toEqual([
      { value: '', label: 'System default' },
      { value: 'ghostty', label: 'Ghostty' },
      { value: 'terminal', label: 'Terminal' },
    ]);
    expect(externalTerminalOptions('ghostty', installed)).toHaveLength(3);
    expect(externalTerminalOptions('iterm2', installed).at(-1)).toEqual({
      value: 'iterm2',
      label: 'iTerm2 (not installed)',
    });
    expect(externalTerminalOptions('foot', []).at(-1)?.label).toBe('foot (not installed)');
  });

  it('names the terminal "Open in …" uses', () => {
    expect(externalTerminalName('terminal', installed, 'X')).toBe('Terminal');
    expect(externalTerminalName(null, installed, 'X')).toBe('Ghostty');
    expect(externalTerminalName('iterm2', installed, 'X')).toBe('iTerm2');
    expect(externalTerminalName(null, [], 'X')).toBe('X');
  });

  it('loads the installed list once', async () => {
    const bridge = fakeBridge({ installed_terminals: installed });
    const store = createTerminalsStore(bridge);
    await store.load();
    await store.load();
    expect(store.installed).toEqual(installed);
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
  });

  it('tries again after a failed load', async () => {
    const bridge = fakeBridge({});
    const store = createTerminalsStore(bridge);
    await store.load();
    expect(store.installed).toEqual([]);
    bridge.replies.installed_terminals = installed;
    await store.load();
    expect(store.installed).toEqual(installed);
  });
});
