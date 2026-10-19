import { afterEach, describe, expect, it } from 'vitest';
import { multiEntryLibrary } from '../testing/fixtures';
import { buttonByText, check, cleanupView, click, renderView, settle, text, type } from '../testing/views';
import type { LibraryDto } from '../types';
import MergeSheet from './MergeSheet.svelte';

afterEach(cleanupView);

/** "box" (c1) shares the host of "Home Server" (e1); the other entries don't. */
function library(): LibraryDto {
  const lib = multiEntryLibrary();
  lib.entries = lib.entries.map((e) => (e.id === 'c1' ? { ...e, host: 'home.example.test' } : e));
  return lib;
}

function render(replies: NonNullable<Parameters<typeof renderView>[2]>['replies'] = {}) {
  let closed = false;
  const view = renderView(
    MergeSheet,
    { targetId: 'e1', onclose: () => (closed = true) },
    { library: library(), replies: { merge_entries: { movedConnections: 2 }, ...replies } },
  );
  return { ...view, closed: () => closed };
}

const rowNames = () => [...document.querySelectorAll('li .name')].map((n) => n.textContent);
const box = (name: string) =>
  [...document.querySelectorAll<HTMLLabelElement>('li label')]
    .find((l) => l.querySelector('.name')?.textContent === name)!
    .querySelector('input')!;

describe('MergeSheet', () => {
  it('lists_other_entries_with_search', () => {
    render();
    expect(text()).toContain('Merge into “Home Server”');
    expect(rowNames()).toEqual(['box', 'db', 'portal', 'router', 'web01']);
    type(document.querySelector<HTMLInputElement>('input[placeholder="Name, host or folder"]')!, 'serv');
    expect(rowNames()).toEqual(['db']);
    type(document.querySelector<HTMLInputElement>('input[placeholder="Name, host or folder"]')!, 'zzz');
    expect(text()).toContain('No entries');
  });

  it('warns_when_host_differs', () => {
    render();
    const warning = (name: string) =>
      box(name).closest('label')!.querySelector('.warning')?.textContent ?? null;
    expect(warning('box')).toBeNull();
    expect(warning('db')).toBe('its connections will use home.example.test');
  });

  it('merge_sends_target_and_checked_sources', async () => {
    const { bridge, toast, closed } = render();
    check(box('db'), true);
    check(box('router'), true);
    click(buttonByText('Merge 2'));
    await settle();
    expect(closed()).toBe(true);
    expect(bridge.invoke).toHaveBeenCalledWith('merge_entries', { target: 'e1', sources: ['c3', 'c4'] });
    expect(toast.current?.message).toBe('Merged — 2 connections moved');
  });

  it('merge_disabled_with_nothing_checked', () => {
    render();
    expect(buttonByText('Merge 0').disabled).toBe(true);
    const selectAll = document.querySelector<HTMLInputElement>('label.checkbox input')!;
    check(selectAll, true);
    expect(buttonByText('Merge 5').disabled).toBe(false);
    check(selectAll, false);
    expect(buttonByText('Merge 0').disabled).toBe(true);
  });
});
