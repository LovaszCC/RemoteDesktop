import { afterEach, describe, expect, it } from 'vitest';
import { multiEntryLibrary } from '../testing/fixtures';
import { buttonByText, check, cleanupView, click, renderView, settle } from '../testing/views';
import MergeSheet from './MergeSheet.svelte';

afterEach(cleanupView);

describe('MergeSheet in its host', () => {
  // MainView passes `targetId={sidebarActions.mergeTarget}` and `onclose` clears it: the prop turns null
  // as soon as the sheet closes, before the merge call reads it.
  it('merges into the target even though closing clears the prop', async () => {
    const props = $state<{ targetId: string; onclose: () => void }>({
      targetId: 'e1',
      onclose: () => {
        props.targetId = null as unknown as string;
      },
    });
    const { bridge } = renderView(MergeSheet, props, {
      library: multiEntryLibrary(),
      replies: { merge_entries: { movedConnections: 1 } },
    });
    const db = [...document.querySelectorAll<HTMLLabelElement>('li label')]
      .find((l) => l.querySelector('.name')?.textContent === 'db')!
      .querySelector('input')!;
    check(db, true);
    click(buttonByText('Merge 1'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('merge_entries', { target: 'e1', sources: ['c3'] });
  });
});
