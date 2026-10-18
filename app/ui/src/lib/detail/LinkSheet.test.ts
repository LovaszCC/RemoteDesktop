import { afterEach, describe, expect, it } from 'vitest';
import { sidebarLibrary } from '../testing/fixtures';
import { buttonByText, check, cleanupView, click, renderView, settle, text, type } from '../testing/views';
import LinkSheet from './LinkSheet.svelte';

afterEach(cleanupView);

describe('LinkSheet', () => {
  it('keeps checked rows across searches and links them', async () => {
    const library = sidebarLibrary();
    const credential = library.credentials[1]!;
    let closed = false;
    const answers: Record<string, string[]> = { '': ['c1', 'c3'], db: ['c3'] };
    const { bridge, toast } = renderView(
      LinkSheet,
      { credential, onclose: () => (closed = true) },
      {
        library,
        replies: {
          link_candidates: (args?: Record<string, unknown>) => answers[args?.query as string] ?? [],
        },
      },
    );
    await settle();
    expect(text()).toContain('Link connections to “Spare”');
    expect(text()).toContain('currently: Domain admin — will move');
    expect(buttonByText('Add 0').disabled).toBe(true);

    const box = () => document.querySelectorAll<HTMLInputElement>('li input[type="checkbox"]')[0]!;
    check(box(), true);
    const search = document.querySelector<HTMLInputElement>(
      'input[placeholder="Name, host, user or folder"]',
    )!;
    type(search, 'db');
    await settle();
    expect(text()).not.toContain('box');
    check(box(), true);
    type(search, 'nothing');
    await settle();
    expect(text()).toContain('No connections');
    expect(buttonByText('Add 2').disabled).toBe(false);

    click(buttonByText('Add 2'));
    await settle();
    expect(closed).toBe(true);
    expect(bridge.invoke).toHaveBeenCalledWith('set_credential', {
      credentialId: 'k2',
      tags: ['c:c1', 'c:c3'],
      connectionIds: null,
    });
    expect(toast.current?.message).toBe('Linked credential set — 1 connections');
  });
});
