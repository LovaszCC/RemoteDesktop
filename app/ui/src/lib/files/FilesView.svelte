<script lang="ts">
  // The Files window (Swift `FileBrowserView`): toolbar (gzip, hidden files, refresh), the remote pane on
  // the left and the local one on the right with → / ← between them, the transfer bar and the conflict
  // dialog. Rows drag to the other pane with the pointer (HTML drag and drop is off where the OS drop
  // handler is on); files dropped from the OS file manager onto the remote pane are uploaded.
  import Archive from '@lucide/svelte/icons/archive';
  import ArrowLeft from '@lucide/svelte/icons/arrow-left';
  import ArrowRight from '@lucide/svelte/icons/arrow-right';
  import Eye from '@lucide/svelte/icons/eye';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import { onMount } from 'svelte';
  import Button from '../components/Button.svelte';
  import IconButton from '../components/IconButton.svelte';
  import { t, tn } from '../i18n/index.svelte';
  import type { FileEntryDto } from '../types';
  import ConflictDialog from './ConflictDialog.svelte';
  import FilePane from './FilePane.svelte';
  import type { FilesStore, Side } from './files.svelte';
  import { connectFailure, type OsDrop } from './model';
  import TransferBar from './TransferBar.svelte';

  interface Props {
    store: FilesStore;
    /** Subscribes to OS file drags (the Tauri window's drag-drop event); returns the unsubscribe. */
    subscribeDrop?: (handler: (event: OsDrop) => void) => Promise<() => void>;
  }

  let { store, subscribeDrop }: Props = $props();

  const status = $derived(store.status);
  const canCopy = $derived(
    store.ready && !store.transferring && status?.state === 'ready' && status.mode !== 'shellOnly',
  );

  /** A pane under a client point. */
  function paneAt(x: number, y: number): Side | null {
    const element = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-pane]');
    const side = element?.dataset.pane;
    return side === 'remote' || side === 'local' ? side : null;
  }

  // Row drag between panes.
  let pending: { side: Side; names: string[]; x: number; y: number } | null = null;
  let drag = $state<{ side: Side; names: string[]; x: number; y: number; over: Side | null } | null>(null);
  let osOver = $state(false);

  function rowPointerDown(side: Side, event: PointerEvent, entry: FileEntryDto) {
    if (event.button !== 0 || !canCopy) return;
    const selection = store.pane(side).selection;
    const names = selection.includes(entry.name) ? [...selection] : [entry.name];
    pending = { side, names, x: event.clientX, y: event.clientY };
  }

  function pointerMove(event: PointerEvent) {
    if (!pending && !drag) return;
    if (pending && !drag) {
      if (Math.hypot(event.clientX - pending.x, event.clientY - pending.y) < 5) return;
      drag = { ...pending, over: null };
      pending = null;
    }
    if (drag) {
      drag.x = event.clientX;
      drag.y = event.clientY;
      drag.over = paneAt(event.clientX, event.clientY);
    }
  }

  function pointerUp(event: PointerEvent) {
    pending = null;
    if (!drag) return;
    const target = paneAt(event.clientX, event.clientY);
    const { side, names } = drag;
    drag = null;
    if (target && target !== side) void store.copy(side, names);
  }

  onMount(() => {
    let off: (() => void) | undefined;
    let disposed = false;
    void subscribeDrop?.((event) => {
      if (event.type === 'leave') {
        osOver = false;
      } else if (event.type === 'over') {
        osOver = paneAt(event.x, event.y) === 'remote';
      } else {
        osOver = false;
        if (paneAt(event.x, event.y) === 'remote' && canCopy) void store.uploadPaths(event.paths);
      }
    }).then((unsubscribe) => {
      if (disposed) unsubscribe();
      else off = unsubscribe;
    });
    return () => {
      disposed = true;
      off?.();
    };
  });

  const selectedRemote = $derived(store.remote.selection);
  const selectedLocal = $derived(store.local.selection);
</script>

<svelte:window onpointermove={pointerMove} onpointerup={pointerUp} />

<div class="files">
  <div class="toolbar">
    <span class="name">{status?.connectionName ?? ''}</span>
    <span class="spacer"></span>
    <IconButton
      icon={Archive}
      label={t(
        'Compress transfers with gzip (faster for text over slow links, can be slower for already compressed files)',
      )}
      active={store.gzip}
      aria-pressed={store.gzip}
      onclick={() => (store.gzip = !store.gzip)}
    />
    <IconButton
      icon={Eye}
      label={t('Show hidden files')}
      active={store.showHidden}
      aria-pressed={store.showHidden}
      onclick={() => void store.setShowHidden(!store.showHidden)}
    />
    <IconButton icon={RefreshCw} label={t('Refresh')} onclick={() => void store.refresh()} />
  </div>

  {#if store.statusError}
    <div class="state" role="alert"><TriangleAlert size={28} />{store.statusError}</div>
  {:else if !status || status.state === 'connecting'}
    <div class="state"><span class="spinner" aria-hidden="true"></span>{t('Connecting…')}</div>
  {:else if status.state === 'failed'}
    <div class="state" role="alert">
      <TriangleAlert size={28} />
      <strong>{t("Can't connect")}</strong>
      <span class="detail selectable">{connectFailure(status)}</span>
      {#if connectFailure(status) !== status.error}<span class="detail small selectable">{status.error}</span
        >{/if}
      <Button variant="primary" onclick={() => void store.reconnect()}>{t('Retry')}</Button>
    </div>
  {:else}
    {#if status.mode !== 'tar'}
      <p class="notice">
        {status.mode === 'sftp'
          ? t('Only single files can be copied: the server has no tar.')
          : t('Copying needs tar or SFTP on the server.')}
      </p>
    {/if}
    <div class="panes">
      <FilePane
        {store}
        side="remote"
        title={t('Remote')}
        highlight={osOver || (drag?.over === 'remote' && drag.side === 'local')}
        onrowpointerdown={(e, entry) => rowPointerDown('remote', e, entry)}
      />
      <div class="buttons">
        <IconButton
          icon={ArrowRight}
          label={t('Download the selected items into the local folder')}
          disabled={!canCopy || selectedRemote.length === 0 || !store.local.listing}
          onclick={() => void store.copy('remote', selectedRemote)}
        />
        <IconButton
          icon={ArrowLeft}
          label={t('Upload the selected items into the remote folder')}
          disabled={!canCopy || selectedLocal.length === 0}
          onclick={() => void store.copy('local', selectedLocal)}
        />
      </div>
      <FilePane
        {store}
        side="local"
        title={t('This computer')}
        highlight={drag?.over === 'local' && drag.side === 'remote'}
        onrowpointerdown={(e, entry) => rowPointerDown('local', e, entry)}
      />
    </div>
  {/if}

  <TransferBar {store} />
</div>

{#if drag}
  <div class="ghost" style:left="{drag.x + 12}px" style:top="{drag.y + 8}px">
    {drag.names.length === 1 ? drag.names[0] : tn('%lld items', drag.names.length)}
  </div>
{/if}

<ConflictDialog {store} />

<style>
  .files {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--bg-content);
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    height: var(--toolbar-height);
    padding: 0 var(--space-3);
    border-bottom: 0.5px solid var(--separator);
    background: var(--bg-toolbar);
  }
  .name {
    font-weight: 600;
  }
  .spacer {
    flex: 1;
  }
  .panes {
    display: grid;
    flex: 1;
    grid-template-columns: minmax(0, 1fr) 36px minmax(0, 1fr);
    min-height: 0;
    padding: var(--space-1);
  }
  .buttons {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    align-items: center;
    justify-content: center;
  }
  .state {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: var(--space-2);
    align-items: center;
    justify-content: center;
    padding: var(--space-5);
    color: var(--fg-secondary);
    text-align: center;
  }
  .state strong {
    color: var(--fg);
  }
  .detail {
    max-width: 560px;
    overflow-wrap: anywhere;
  }
  .small {
    font-size: var(--font-size-small);
  }
  .notice {
    margin: 0;
    padding: 6px var(--space-3);
    background: var(--warning-soft);
    color: var(--warning);
    font-size: var(--font-size-callout);
  }
  .spinner {
    width: 18px;
    height: 18px;
    border: 2px solid var(--bg-selected);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .ghost {
    position: fixed;
    z-index: 100;
    max-width: 260px;
    overflow: hidden;
    padding: 2px 8px;
    border-radius: var(--radius-small);
    background: var(--accent);
    color: var(--accent-fg);
    box-shadow: var(--shadow-popover);
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none;
  }
</style>
