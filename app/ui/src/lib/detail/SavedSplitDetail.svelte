<script lang="ts">
  import Grid2x2 from '@lucide/svelte/icons/grid-2x2';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Play from '@lucide/svelte/icons/play';
  import Trash from '@lucide/svelte/icons/trash';
  import Button from '../components/Button.svelte';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import type { ConnectionDto, SavedSplitDto } from '../types';
  import { viewDeps } from './deps';
  import DetailPage from './DetailPage.svelte';

  /** Swift `SavedSplitDetailView`: its panes and the actions. */
  let { split }: { split: SavedSplitDto } = $props();
  const deps = viewDeps();

  const connections = $derived(
    split.connectionIds
      .map((id) => deps.library.connection(id))
      .filter((c): c is ConnectionDto => c !== undefined),
  );
  const missing = $derived(split.connectionIds.length - connections.length);
</script>

<DetailPage title={split.name} subtitle={t('Saved split · %lld terminals', connections.length)}>
  {#snippet icon()}
    <span class="split-icon"><Grid2x2 size={30} strokeWidth={1.5} /></span>
  {/snippet}

  <div class="actions">
    <Button
      variant="primary"
      disabled={connections.length === 0}
      onclick={() => deps.actions.openSavedSplit(split.id)}
    >
      <Play size={13} />{t('Open')}
    </Button>
    <Button disabled={deps.readOnly} onclick={() => deps.actions.renameSavedSplit(split.id)}>
      <Pencil size={13} />{t('Rename…')}
    </Button>
    <Button
      variant="destructive"
      disabled={deps.readOnly}
      onclick={() => deps.actions.deleteSavedSplit(split.id)}
    >
      <Trash size={13} />{t('Delete')}
    </Button>
  </div>

  <section>
    <h2>{t('Terminals')}</h2>
    <ul>
      {#each connections as connection, index (index)}
        <li><ConnectionIcon type={connection.type} />{connection.name}</li>
      {/each}
    </ul>
    {#if missing > 0}
      <p class="muted">{t('%lld deleted connections are left out.', missing)}</p>
    {/if}
    <p class="caption">{t('Broadcast is always off when the split opens.')}</p>
  </section>
</DetailPage>

<style>
  .split-icon {
    display: inline-flex;
    color: var(--label-green);
  }
  ul {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  p {
    margin: 0;
  }
</style>
