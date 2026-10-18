import { createRawSnippet, mount, tick, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import Modal from './Modal.svelte';

const form = createRawSnippet(() => ({ render: () => '<div><p>Long page</p><input class="field" /></div>' }));

let mounted: ReturnType<typeof mount> | null = null;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = null;
  document.body.innerHTML = '';
});

describe('Modal initial focus', () => {
  it('focuses the first field of a form', async () => {
    mounted = mount(Modal, { target: document.body, props: { title: 'Form', children: form } });
    await tick();
    await tick();
    expect(document.activeElement?.classList.contains('field')).toBe(true);
  });

  it('focuses the sheet itself when asked (long pages like Settings)', async () => {
    mounted = mount(Modal, {
      target: document.body,
      props: { title: 'Settings', children: form, initialFocus: 'sheet' },
    });
    await tick();
    await tick();
    expect(document.activeElement?.getAttribute('role')).toBe('dialog');
  });
});
