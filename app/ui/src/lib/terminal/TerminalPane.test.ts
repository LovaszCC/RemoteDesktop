import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import TerminalPane from './TerminalPane.svelte';
import source from './TerminalPane.svelte?raw';
import type { TerminalHandle } from './xterm';

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

// jsdom has no ResizeObserver.
globalThis.ResizeObserver ??= class {
  observe() {}
  disconnect() {}
  unobserve() {}
} as unknown as typeof ResizeObserver;

function fakeHandle(): TerminalHandle {
  return {
    write: vi.fn(),
    clearLocal: vi.fn(),
    attach: vi.fn(),
    detach: vi.fn(),
    fit: vi.fn(),
    size: () => ({ cols: 80, rows: 24 }),
    focus: vi.fn(),
    dispose: vi.fn(),
  };
}

describe('TerminalPane', () => {
  it('attaches the terminal on mount and detaches it on unmount', () => {
    const handle = fakeHandle();
    mounted = mount(TerminalPane, { target: document.body, props: { handle, active: false } });
    flushSync();
    const pane = document.querySelector<HTMLElement>('.pane');
    expect(handle.attach).toHaveBeenCalledWith(pane);
    unmount(mounted);
    mounted = undefined;
    expect(handle.detach).toHaveBeenCalledWith(pane);
  });

  // xterm leaves its helper textarea at the old cursor row when the pane shrinks; unclipped it overflowed
  // the panel's scrollable content, which a wheel at the end of the scrollback then scrolled (the session
  // toolbar vanished above, an empty band showed below).
  // jsdom gets no component CSS, so the rule is read from the source.
  it('clips what xterm draws outside it, so nothing can scroll the panel', () => {
    const rule = /\n {2}\.pane \{([^}]*)\}/.exec(source)?.[1] ?? '';
    expect(rule).toMatch(/^\s*overflow: clip;$/m);
  });
});
