<script lang="ts">
  // Swift `ConflictSheet`: the destination exists; Replace (a folder merges) or Skip, optionally for the
  // rest of the transfer. Esc skips.
  import Button from '../components/Button.svelte';
  import Checkbox from '../components/Checkbox.svelte';
  import Modal from '../components/Modal.svelte';
  import { t } from '../i18n/index.svelte';
  import type { FilesStore } from './files.svelte';

  let { store }: { store: FilesStore } = $props();
  let applyToAll = $state(false);
  const conflict = $derived(store.conflict);
</script>

{#if conflict}
  {#key conflict.requestId}
    <Modal
      title={t('“%@” already exists', conflict.name)}
      onclose={() => void store.answerConflict('skip', applyToAll)}
      width={420}
    >
      <p class="message">
        {conflict.isFolder
          ? t('Replacing a folder merges it: files with the same name are overwritten, other files stay.')
          : t('Replacing overwrites the existing file.')}
      </p>
      <Checkbox bind:checked={applyToAll} label={t('Apply to all')} />
      {#snippet footer()}
        <Button onclick={() => void store.answerConflict('skip', applyToAll)}>{t('Skip')}</Button>
        <Button variant="primary" onclick={() => void store.answerConflict('replace', applyToAll)}>
          {t('Replace')}
        </Button>
      {/snippet}
    </Modal>
  {/key}
{/if}

<style>
  .message {
    margin: 0 0 var(--space-3);
    color: var(--fg-secondary);
  }
</style>
