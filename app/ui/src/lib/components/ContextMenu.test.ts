import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ContextMenu from './ContextMenu.svelte';
import type { MenuItem } from './menu.svelte';

let component: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  document.body.innerHTML = '';
});

function key(target: Element, k: string) {
  target.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true }));
  flushSync();
}

async function open(items: MenuItem[], onclose = vi.fn()) {
  component = mount(ContextMenu, { target: document.body, props: { items, x: 10, y: 10, onclose } });
  await tick();
  await tick();
  return document.querySelector('.context-menu')!;
}

describe('ContextMenu', () => {
  it('moves with the arrows past separators and disabled rows, and acts on Enter', async () => {
    const first = vi.fn();
    const third = vi.fn();
    const onclose = vi.fn();
    const menu = await open(
      [
        { label: 'First', action: first },
        { kind: 'separator' },
        { label: 'Disabled', disabled: true },
        { label: 'Third', action: third },
      ],
      onclose,
    );
    key(menu, 'ArrowDown');
    expect(menu.querySelector('.active')?.textContent).toContain('First');
    key(menu, 'ArrowDown');
    expect(menu.querySelector('.active')?.textContent).toContain('Third');
    key(menu, 'ArrowDown');
    expect(menu.querySelector('.active')?.textContent).toContain('First');
    key(menu, 'ArrowUp');
    key(menu, 'Enter');
    expect(onclose).toHaveBeenCalled();
    expect(third).toHaveBeenCalled();
    expect(first).not.toHaveBeenCalled();
  });

  it('opens a submenu with → and closes it with ←', async () => {
    const pick = vi.fn();
    const menu = await open([{ label: 'Color', submenu: [{ label: 'Red', action: pick }] }]);
    key(menu, 'ArrowDown');
    key(menu, 'ArrowRight');
    for (let i = 0; i < 5; i++) await tick();
    flushSync();
    const menus = document.querySelectorAll('.context-menu');
    expect(menus).toHaveLength(2);
    const submenu = menus[1]!;
    expect(submenu.querySelector('.active')?.textContent).toContain('Red');
    key(submenu, 'ArrowLeft');
    expect(document.querySelectorAll('.context-menu')).toHaveLength(1);
  });

  it('closes on Escape and on a click outside', async () => {
    const onclose = vi.fn();
    const menu = await open([{ label: 'One' }], onclose);
    key(menu, 'Escape');
    expect(onclose).toHaveBeenCalledTimes(1);
    document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    expect(onclose).toHaveBeenCalledTimes(2);
  });
});
