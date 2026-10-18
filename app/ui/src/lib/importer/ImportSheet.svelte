<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Minus from '@lucide/svelte/icons/minus';
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import { IMPORT_TITLES, skippedLabel, type ImportStore } from './importer.svelte';

  /** Swift `WinboxImportSheet`, for both importers: pick the entries, duplicates start unchecked. */
  let { importer }: { importer: ImportStore } = $props();

  const draft = $derived(importer.draft);
  const preview = $derived(draft?.preview);
  const duplicates = $derived(preview?.rows.filter((r) => r.duplicate).length ?? 0);
  const skipped = $derived(preview?.skipped.map(skippedLabel) ?? []);
</script>

{#if draft && preview}
  <Modal title={t(IMPORT_TITLES[draft.kind])} onclose={() => importer.close()} width={760}>
    <div class="import-sheet">
      <p class="summary">
        <span class="file">{preview.fileName}</span>
        {t('Entries: %lld, already in the library: %lld', preview.rows.length, duplicates)}
      </p>
      {#if preview.withoutPassword > 0}
        <p class="note">
          {t(
            '%lld entries had an encrypted password in the export; enter those by hand.',
            preview.withoutPassword,
          )}
        </p>
      {/if}
      {#if skipped.length > 0}
        <p class="note">{t('Skipped (%lld): %@', skipped.length, skipped.slice(0, 15).join(', '))}</p>
      {/if}
      <div class="table">
        <div class="row head" aria-hidden="true">
          <span></span>
          <span>{t('Name')}</span>
          <span>{t('Address')}</span>
          <span>{t('User')}</span>
          <span>{t('Group')}</span>
          <span class="center">{t('Password')}</span>
        </div>
        {#each preview.rows as row (row.id)}
          <label class="row" data-id={row.id}>
            <span>
              <input
                type="checkbox"
                checked={draft.included.has(row.id)}
                onchange={(e) => importer.setIncluded(row.id, e.currentTarget.checked)}
              />
            </span>
            <span class="name">
              {#if row.type === 'credential'}
                <span class="credential-icon"><KeyRound size={14} strokeWidth={1.75} /></span>
              {:else}
                <ConnectionIcon type={row.type} />
              {/if}
              <span class="text">{row.name}</span>
              {#if row.duplicate}<span class="capsule">{t('already exists')}</span>{/if}
            </span>
            <span class="muted text">{row.host}{row.port === null ? '' : `:${row.port}`}</span>
            <span class="muted text">{row.user}</span>
            <span class="muted text">{row.groupPath.join(' / ')}</span>
            <span class="muted center" aria-label={row.hasPassword ? t('Password') : ''}>
              {#if row.hasPassword}<Check size={13} />{:else}<Minus size={13} />{/if}
            </span>
          </label>
        {/each}
      </div>
    </div>
    {#snippet footer()}
      <Button onclick={() => importer.setAll(true)}>{t('Select all')}</Button>
      <Button onclick={() => importer.setAll(false)}>{t('Select none')}</Button>
      <span class="spacer"></span>
      <Button onclick={() => importer.close()}>{t('Cancel')}</Button>
      <Button
        variant="primary"
        disabled={importer.includedCount === 0 || importer.busy}
        onclick={() => void importer.commit()}
      >
        {t('Import %lld', importer.includedCount)}
      </Button>
    {/snippet}
  </Modal>
{/if}

<style>
  .import-sheet {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    height: 420px;
  }
  p {
    margin: 0;
  }
  .summary {
    display: flex;
    gap: var(--space-2);
    color: var(--fg-secondary);
  }
  .file {
    color: var(--fg);
    font-weight: 600;
  }
  .note {
    color: var(--fg-secondary);
    font-size: var(--font-size-small);
  }
  .table {
    flex: 1;
    min-height: 0;
    padding: 4px;
    overflow: auto;
    border: 0.5px solid var(--separator);
    border-radius: var(--radius);
  }
  .row {
    display: grid;
    grid-template-columns:
      22px minmax(140px, 2fr) minmax(100px, 1.4fr) minmax(70px, 1fr) minmax(80px, 1.2fr)
      60px;
    align-items: center;
    gap: 6px;
    padding: 3px 4px;
    border-radius: var(--radius-small);
  }
  label.row:hover {
    background: var(--bg-hover);
  }
  .head {
    color: var(--fg-secondary);
    font-size: var(--font-size-small);
    font-weight: 600;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted {
    color: var(--fg-secondary);
  }
  .center {
    display: flex;
    justify-content: center;
  }
  .credential-icon {
    display: inline-flex;
    flex: none;
    justify-content: center;
    width: 18px;
    color: var(--label-gray);
  }
  input {
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--accent);
  }
  .capsule {
    flex: none;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--bg-selected);
    font-size: var(--font-size-small);
  }
  .spacer {
    flex: 1;
  }
</style>
