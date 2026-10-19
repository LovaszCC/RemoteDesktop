<script lang="ts">
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from '../components/Button.svelte';
  import Checkbox from '../components/Checkbox.svelte';
  import Modal from '../components/Modal.svelte';
  import TextField from '../components/TextField.svelte';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import { viewDeps } from './deps';
  import { NO_PICKS, allPicked, mergeCandidates, setAllPicked, setPicked } from './model';

  /**
   * "Merge…": a searchable checklist of the other entries; their connections move into `targetId`, whose
   * shared fields and default win. Rows whose host differs say so. Picks stay when the search hides them.
   */
  let { targetId, onclose }: { targetId: string; onclose: () => void } = $props();
  const deps = viewDeps();

  let query = $state('');
  let picked = $state<ReadonlySet<string>>(NO_PICKS);

  const target = $derived(deps.library.entry(targetId));
  const rows = $derived(mergeCandidates(deps.library.library, targetId, query));
  const shownIds = $derived(rows.map((row) => row.entry.id));

  async function merge() {
    // Read the props before closing: the host clears `targetId` when the sheet closes.
    const target = targetId;
    const sources = [...picked];
    onclose();
    await deps.actions.mergeEntries(target, sources);
  }
</script>

<Modal title={t('Merge into “%@”', target?.name ?? '')} {onclose} width={560}>
  <div class="merge-sheet">
    <p class="caption">
      {t('The connections of the checked entries move into this entry; the emptied entries are deleted.')}
    </p>
    <div class="filter">
      <TextField
        bind:value={query}
        placeholder={t('Name, host or folder')}
        aria-label={t('Search')}
        class="search"
      />
      <Checkbox
        label={t('Select all')}
        checked={allPicked(picked, shownIds)}
        disabled={shownIds.length === 0}
        onchange={(on) => (picked = setAllPicked(picked, shownIds, on))}
      />
    </div>
    <ul>
      {#each rows as { entry, hostDiffers } (entry.id)}
        {@const type = deps.library.connection(entry.defaultConnectionId ?? entry.connectionIds[0])?.type}
        <li>
          <label>
            <input
              type="checkbox"
              checked={picked.has(entry.id)}
              onchange={(e) => (picked = setPicked(picked, entry.id, e.currentTarget.checked))}
            />
            {#if type}<ConnectionIcon {type} />{/if}
            <span class="name">{entry.name}</span>
            <span class="muted">{entry.host}</span>
            {#if entry.folderPath}<span class="muted">· {entry.folderPath}</span>{/if}
            {#if hostDiffers && target}
              <span class="warning"
                ><TriangleAlert size={12} />{t('its connections will use %@', target.host)}</span
              >
            {/if}
          </label>
        </li>
      {:else}
        <li class="empty">{t('No entries')}</li>
      {/each}
    </ul>
  </div>
  {#snippet footer()}
    <Button onclick={onclose}>{t('Cancel')}</Button>
    <Button variant="primary" disabled={picked.size === 0} onclick={merge}
      >{t('Merge %lld', picked.size)}</Button
    >
  {/snippet}
</Modal>

<style>
  .merge-sheet {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    height: 380px;
  }
  .caption {
    margin: 0;
    color: var(--fg-secondary);
    font-size: var(--font-size-small);
  }
  .filter {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .filter :global(.search) {
    flex: 1;
  }
  ul {
    flex: 1;
    margin: 0;
    padding: 4px;
    overflow: auto;
    border: 0.5px solid var(--separator);
    border-radius: var(--radius);
    list-style: none;
  }
  label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 4px;
    border-radius: var(--radius-small);
  }
  label:hover {
    background: var(--bg-hover);
  }
  input {
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--accent);
  }
  .muted {
    overflow: hidden;
    color: var(--fg-secondary);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .warning {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    margin-left: auto;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--bg-selected);
    color: var(--label-orange);
    font-size: var(--font-size-small);
    white-space: nowrap;
  }
  .empty {
    padding: var(--space-5);
    color: var(--fg-secondary);
    text-align: center;
  }
</style>
