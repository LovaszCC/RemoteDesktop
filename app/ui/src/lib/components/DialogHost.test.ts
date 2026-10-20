import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { createDialogStore } from '../stores/dialogs.svelte';
import { buttonByText, check, click, text } from '../testing/views';
import DialogHost from './DialogHost.svelte';

let mounted: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

function render() {
  const dialogs = createDialogStore();
  mounted = mount(DialogHost, { target: document.body, props: { dialogs } });
  return dialogs;
}

const checkbox = () => document.querySelector<HTMLInputElement>('input[type="checkbox"]');

describe('DialogHost confirmations', () => {
  it('shows the checkbox unchecked and passes its state on confirm', async () => {
    const dialogs = render();
    const answer = dialogs.confirmWithOption({
      title: 'Delete “Office”?',
      message: 'Subfolders are deleted too.',
      confirmLabel: 'Delete',
      destructive: true,
      option: 'Also delete the 2 entries inside',
    });
    flushSync();
    expect(text()).toContain('Also delete the 2 entries inside');
    expect(checkbox()!.checked).toBe(false);
    check(checkbox()!, true);
    click(buttonByText('Delete'));
    expect(await answer).toEqual({ checked: true });
  });

  it('has no checkbox without an option', async () => {
    const dialogs = render();
    const answer = dialogs.confirm({ title: 'Sure?', confirmLabel: 'OK' });
    flushSync();
    expect(checkbox()).toBeNull();
    click(buttonByText('OK'));
    expect(await answer).toBe(true);
  });
});
