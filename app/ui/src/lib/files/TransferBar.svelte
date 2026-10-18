<script lang="ts">
  // The bottom bar: the running transfer (item, n of m, bytes, a progress bar and Cancel), then the last
  // result (copied / skipped / failed with the errors) until dismissed.
  import X from '@lucide/svelte/icons/x';
  import Button from '../components/Button.svelte';
  import IconButton from '../components/IconButton.svelte';
  import { i18n, t } from '../i18n/index.svelte';
  import type { FilesStore } from './files.svelte';
  import { doneErrors, doneSummary, progressBytes, progressFraction, progressTitle } from './model';

  let { store }: { store: FilesStore } = $props();
  const progress = $derived(store.progress);
  const fraction = $derived(progress ? progressFraction(progress) : null);
  const done = $derived(store.lastDone);
</script>

{#if store.transferring}
  <footer class="bar" aria-live="polite">
    <div class="info">
      <div class="line">
        <span class="title">{progress ? progressTitle(progress) : t('Connecting…')}</span>
        {#if progress}<span class="bytes">{progressBytes(progress, i18n.locale)}</span>{/if}
      </div>
      <div
        class="track"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}
      >
        <div
          class="fill"
          class:indeterminate={fraction === null}
          style:width="{(fraction ?? 0.3) * 100}%"
        ></div>
      </div>
    </div>
    <Button onclick={() => void store.cancel()}>{t('Cancel')}</Button>
  </footer>
{:else if done || store.actionError}
  <footer class="bar result" class:failed={!!store.actionError || (done && doneErrors(done).length > 0)}>
    <div class="info">
      {#if done}
        <span class="title">{doneSummary(done)}</span>
        {#each doneErrors(done) as error (error)}
          <span class="error selectable">{error}</span>
        {/each}
      {/if}
      {#if store.actionError}<span class="error">{store.actionError}</span>{/if}
    </div>
    <IconButton icon={X} label={t('Dismiss')} onclick={() => store.dismissResult()} />
  </footer>
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-top: 0.5px solid var(--separator);
    background: var(--bg-toolbar);
  }
  .info {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .line {
    display: flex;
    gap: var(--space-2);
    justify-content: space-between;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bytes {
    color: var(--fg-secondary);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .track {
    overflow: hidden;
    height: 6px;
    border-radius: 3px;
    background: var(--bg-selected);
  }
  .fill {
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transition: width 0.2s;
  }
  .indeterminate {
    animation: pulse 1.2s ease-in-out infinite alternate;
  }
  @keyframes pulse {
    from {
      margin-left: 0;
    }
    to {
      margin-left: 70%;
    }
  }
  .error {
    color: var(--danger);
    font-size: var(--font-size-callout);
    overflow-wrap: anywhere;
  }
</style>
