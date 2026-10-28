import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { TabSession } from '../tabs.svelte';
import SplitTabsDialog from './SplitTabsDialog.svelte';

let mounted: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

function render(count: number, initial: string[] = []) {
  const candidates: TabSession[] = Array.from({ length: count }, (_, i) => ({
    id: `s${i}`,
    title: `server ${i}`,
    type: 'ssh',
    color: null,
  }));
  const onconfirm = vi.fn();
  const oncancel = vi.fn();
  mounted = mount(SplitTabsDialog, {
    target: document.body,
    props: { candidates, titleOf: (s: TabSession) => s.title.toUpperCase(), initial, onconfirm, oncancel },
  });
  flushSync();
  const boxes = () => [...document.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')];
  const button = (label: string) =>
    [...document.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label)!;
  const tick = (i: number) => {
    boxes()[i]!.click();
    flushSync();
  };
  return { boxes, button, tick, onconfirm, oncancel };
}

describe('SplitTabsDialog', () => {
  it('lists the tabs by their shown title, ticks the initial ones and needs two', () => {
    const { boxes, button, tick, onconfirm } = render(3, ['s1', 'gone']);
    expect(document.body.textContent).toContain('SERVER 0');
    expect(boxes().map((b) => b.checked)).toEqual([false, true, false]);
    expect(button('Split').disabled).toBe(true);
    tick(0);
    button('Split').click();
    expect(onconfirm).toHaveBeenCalledWith(['s1', 's0']);
  });

  it('allows at most four ticks', () => {
    const { boxes, tick } = render(5);
    for (const i of [0, 1, 2, 3]) tick(i);
    expect(boxes().map((b) => b.disabled)).toEqual([false, false, false, false, true]);
    tick(1);
    expect(boxes().map((b) => b.disabled)).toEqual([false, false, false, false, false]);
  });

  it('cancels', () => {
    const { button, oncancel, onconfirm } = render(2);
    button('Cancel').click();
    expect(oncancel).toHaveBeenCalledOnce();
    expect(onconfirm).not.toHaveBeenCalled();
  });
});
