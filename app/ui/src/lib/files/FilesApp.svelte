<script lang="ts">
  // The page of a Files window (label `files-<connection id>`): the language from the settings, the
  // browser's store, the host key prompt of its connection, and the OS drop events of this window.
  import { onMount } from 'svelte';
  import HostKeyDialog from '../forwards/HostKeyDialog.svelte';
  import { t } from '../i18n/index.svelte';
  import { settings } from '../stores/settings.svelte';
  import { createFilesStore } from './files.svelte';
  import FilesView from './FilesView.svelte';
  import type { OsDrop } from './model';

  let { browserId }: { browserId: string } = $props();
  // The window shows one browser for its whole life.
  // svelte-ignore state_referenced_locally
  const store = createFilesStore(browserId);

  /** Tauri's drag-drop events of this window, in logical client coordinates. */
  async function subscribeDrop(handler: (event: OsDrop) => void) {
    const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow');
    return getCurrentWebviewWindow().onDragDropEvent((event) => {
      const payload = event.payload;
      const scale = window.devicePixelRatio || 1;
      if (payload.type === 'leave') handler({ type: 'leave' });
      else if (payload.type === 'drop')
        handler({
          type: 'drop',
          x: payload.position.x / scale,
          y: payload.position.y / scale,
          paths: payload.paths,
        });
      else handler({ type: 'over', x: payload.position.x / scale, y: payload.position.y / scale });
    });
  }

  onMount(() => {
    void settings.start().catch(() => {});
    void store.start();
    return () => {
      store.stop();
      settings.stop();
    };
  });
</script>

<FilesView {store} {subscribeDrop} />
<HostKeyDialog store={store.hostKeys} acceptLabel={t('Accept and Connect')} />
