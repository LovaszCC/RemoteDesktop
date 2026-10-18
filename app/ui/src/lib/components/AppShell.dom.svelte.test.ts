import { flushSync, mount, unmount } from 'svelte';
import { createRawSnippet } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import AppShell from './AppShell.svelte';

const text = (value: string) => createRawSnippet(() => ({ render: () => `<p>${value}</p>` }));

let mounted: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = null;
  document.body.innerHTML = '';
});

describe('AppShell collapsed sidebar', () => {
  it('hides the sidebar from the layout and from focus', () => {
    let collapsed = $state(false);
    const layout = {
      get collapsed() {
        return collapsed;
      },
      set: (value: boolean) => (collapsed = value),
    };
    mounted = mount(AppShell, {
      target: document.body,
      props: { sidebar: text('side'), children: text('main'), layout },
    });
    const shell = document.querySelector<HTMLElement>('.shell')!;
    const aside = document.querySelector<HTMLElement>('aside')!;
    expect(shell.style.getPropertyValue('--sidebar-width')).not.toBe('0px');
    expect(aside.inert).toBe(false);
    layout.set(true);
    flushSync();
    expect(shell.classList.contains('collapsed')).toBe(true);
    expect(shell.style.getPropertyValue('--sidebar-width')).toBe('0px');
    expect(aside.inert).toBe(true);
    // ArrowRight on the edge brings it back.
    document
      .querySelector<HTMLElement>('[role="separator"]')!
      .dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    flushSync();
    expect(collapsed).toBe(false);
  });
});
