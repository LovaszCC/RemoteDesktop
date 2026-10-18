import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { contextMenu, closeContextMenu } from '../components/menu.svelte';
import { createSettingsStore } from '../stores/settings.svelte';
import { TabsStore } from '../tabs.svelte';
import { fakeBridge } from '../testing/bridge';
import TabBar from './TabBar.svelte';

let mounted: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  closeContextMenu();
  document.body.innerHTML = '';
});

function render(canSplit = false) {
  const tabs = new TabsStore();
  tabs.openSession({ id: 'a', title: 'web01', type: 'ssh', color: 'red' });
  tabs.openSession({ id: 'b', title: 'db01', type: 'rdp', color: null });
  const settings = createSettingsStore({ ...fakeBridge(), onLanguage: () => {} });
  const splits = {
    canSplitActive: canSplit,
    splitActive: vi.fn(async () => {}),
    save: vi.fn(async () => {}),
    canUpdateSaved: vi.fn(() => true),
    updateSaved: vi.fn(async () => {}),
  };
  mounted = mount(TabBar, { target: document.body, props: { tabs, settings, splits } });
  flushSync();
  const tab = (i: number) => document.querySelectorAll<HTMLElement>('[role="tab"]')[i]!;
  return { tabs, tab, splits };
}

describe('TabBar', () => {
  it('shows Details first, then the sessions, with the active one selected', () => {
    const { tabs, tab } = render();
    expect(tab(0).textContent).toContain('Details');
    expect(tab(1).textContent).toContain('web01');
    expect(tab(2).getAttribute('aria-selected')).toBe('true');
    tab(0).click();
    flushSync();
    expect(tabs.active).toBeNull();
    expect(tab(1).querySelector('.underline')).not.toBeNull();
    expect(tab(2).querySelector('.underline')).toBeNull();
  });

  it('renames inline: Enter saves, Escape cancels, the tooltip shows the automatic title', async () => {
    const { tabs, tab } = render();
    tab(1).dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    await Promise.resolve();
    flushSync();
    let input = tab(1).querySelector('input')!;
    input.value = '  prod  ';
    input.dispatchEvent(new Event('input'));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    flushSync();
    expect(tabs.displayTitle(tabs.session('a')!)).toBe('prod');
    expect(tab(1).title).toBe('web01');

    tab(1).dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    flushSync();
    input = tab(1).querySelector('input')!;
    input.value = 'other';
    input.dispatchEvent(new Event('input'));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    flushSync();
    expect(tabs.displayTitle(tabs.session('a')!)).toBe('prod');
  });

  it('offers rename and the colour submenu, and closes a tab', () => {
    const { tabs, tab } = render();
    tab(1).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 5, clientY: 5 }));
    const items = contextMenu.open!.items;
    expect(items.map((i) => ('label' in i ? i.label : '-'))).toEqual(['Rename…', 'Color']);
    const colors = 'submenu' in items[1]! ? items[1].submenu! : [];
    expect(colors.map((i) => ('label' in i ? i.label : '-'))).toEqual([
      'Entry color',
      '-',
      'None',
      'Red',
      'Orange',
      'Yellow',
      'Green',
      'Blue',
      'Purple',
      'Pink',
      'Gray',
    ]);
    const none = colors[2]!;
    if ('action' in none) none.action?.();
    expect(tabs.displayColor(tabs.session('a')!)).toBeNull();
    const reset = colors[0]!;
    if ('action' in reset) reset.action?.();
    expect(tabs.displayColor(tabs.session('a')!)).toBe('red');

    tab(2).querySelector<HTMLButtonElement>('.close')!.click();
    flushSync();
    expect(tabs.ids).toEqual(['a']);
    expect(tabs.active).toBe('a');
  });

  it('marks a root session with a warning icon and an orange tint', () => {
    const { tabs, tab } = render();
    expect(tab(2).classList.contains('root')).toBe(false);
    tabs.updateSession('b', { root: true });
    flushSync();
    expect(tab(2).classList.contains('root')).toBe(true);
    expect(tab(2).querySelector('.root-icon')?.getAttribute('title')).toBe(
      'Root: commands run with full privileges',
    );
    expect(tab(2).style.background).toContain('--label-orange');
    // A tab colour wins over the tint.
    tabs.updateSession('a', { root: true });
    flushSync();
    expect(tab(1).style.background).toContain('--label-red');
  });

  it('shows split tabs with their own icon, menu and orange broadcast tint', () => {
    const { tabs, tab, splits } = render();
    tabs.openSession({ id: 'T', title: 'a + b', type: 'ssh', color: null, split: true });
    flushSync();
    expect(tab(3).querySelector('.split-icon')).not.toBeNull();
    expect(tab(3).classList.contains('broadcasting')).toBe(false);
    tabs.updateSession('T', { broadcasting: true });
    flushSync();
    expect(tab(3).classList.contains('broadcasting')).toBe(true);
    expect(tab(3).style.background).toContain('--label-orange');

    tab(3).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 5, clientY: 5 }));
    const items = contextMenu.open!.items;
    expect(items.map((i) => ('label' in i ? i.label : '-'))).toEqual([
      'Rename…',
      'Save split…',
      'Update saved split',
      'Color',
    ]);
    for (const item of items.slice(1, 3)) if ('action' in item) item.action?.();
    expect(splits.save).toHaveBeenCalledWith('T');
    expect(splits.updateSaved).toHaveBeenCalledWith('T');
  });

  it('offers Split terminal when the active tab can split', () => {
    expect(document.querySelector('.split-button')).toBeNull();
    const { splits } = render(true);
    document.querySelector<HTMLButtonElement>('.split-button')!.click();
    expect(splits.splitActive).toHaveBeenCalled();
  });
});
