<script lang="ts">
  import Download from '@lucide/svelte/icons/download';
  import FileUp from '@lucide/svelte/icons/file-up';
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import { t } from '../i18n/index.svelte';
  import { csvSkippedLabel, type CsvImportStore } from './csvImporter.svelte';
  import TransferRows from './TransferRows.svelte';

  /** CSV import: the columns and the sample file, then the chosen file's rows. */
  let { importer }: { importer: CsvImportStore } = $props();

  const draft = $derived(importer.draft);
  const preview = $derived(draft?.preview);
  const conflicts = $derived(preview?.rows.filter((r) => r.conflict).length ?? 0);
  const allIds = $derived(preview?.rows.map((r) => r.id) ?? []);
  const skipped = $derived(preview?.skipped.map(csvSkippedLabel) ?? []);

  const COLUMNS: [string, string][] = [
    ['folder', 'Folder path, e.g. Clients/Acme; empty means the top level.'],
    ['name', 'Rows with the same name become one entry with several connections.'],
    ['host', 'Host name or IP address; for web the address, e.g. https://intranet:8443/app.'],
    ['type', 'rdp, ssh, web or mikrotik.'],
  ];
</script>

{#if importer.open}
  <Modal title={t('Import from CSV')} onclose={() => importer.close()} width={draft ? 760 : 520}>
    {#if !draft || !preview}
      <div class="intro">
        <p>
          {t(
            'Collect your servers in a spreadsheet with these columns, save it as CSV and choose it here. Logins and passwords can be set afterwards for many entries at once.',
          )}
        </p>
        <dl>
          {#each COLUMNS as [name, help] (name)}
            <dt>{t(name)}</dt>
            <dd>{t(help)}</dd>
          {/each}
        </dl>
        <p class="note">{t('The sample shows every case. Separator: ; or , — save as "CSV UTF-8".')}</p>
        <div class="actions">
          <Button onclick={() => void importer.saveSample()} disabled={importer.busy}>
            <Download size={14} />
            {t('Download sample…')}
          </Button>
          <Button variant="primary" onclick={() => void importer.choose()} disabled={importer.busy}>
            <FileUp size={14} />
            {t('Choose file…')}
          </Button>
        </div>
      </div>
    {:else}
      <div class="import-sheet">
        <p class="summary">
          <span class="file">{preview.fileName}</span>
          {t('Entries: %lld, already in the library: %lld', preview.rows.length, conflicts)}
        </p>
        {#if conflicts > 0}
          <p class="note">
            {t('Some items already exist: choose Overwrite or Keep both for each, or uncheck them.')}
          </p>
        {/if}
        {#if skipped.length > 0}
          <p class="note skipped">
            {t('Skipped (%lld): %@', skipped.length, skipped.slice(0, 15).join(', '))}
          </p>
        {/if}
        <TransferRows
          rows={preview.rows}
          included={draft.included}
          choices={draft.choices}
          logins={false}
          oninclude={(ids, on) => importer.setIncluded(ids, on)}
          onchoice={(id, choice) => importer.setChoice(id, choice)}
        />
      </div>
    {/if}
    {#snippet footer()}
      {#if draft}
        <Button onclick={() => importer.setIncluded(allIds, true)}>{t('Select all')}</Button>
        <Button onclick={() => importer.setIncluded(allIds, false)}>{t('Select none')}</Button>
        <Button onclick={() => void importer.choose()} disabled={importer.busy}>
          {t('Choose another file…')}
        </Button>
      {/if}
      <span class="spacer"></span>
      <Button onclick={() => importer.close()}>{t('Cancel')}</Button>
      {#if draft}
        <Button
          variant="primary"
          disabled={importer.includedCount === 0 || importer.unanswered > 0 || importer.busy}
          onclick={() => void importer.commit()}
        >
          {t('Import %lld', importer.includedCount)}
        </Button>
      {/if}
    {/snippet}
  </Modal>
{/if}

<style>
  .intro {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  p {
    margin: 0;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px var(--space-3);
    margin: 0;
  }
  dt {
    font-family: var(--font-mono, ui-monospace, monospace);
    font-weight: 600;
  }
  dd {
    margin: 0;
    color: var(--fg-secondary);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
  }
  .import-sheet {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    height: 420px;
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
  .skipped {
    max-height: 3.6em;
    overflow: auto;
  }
  .spacer {
    flex: 1;
  }
</style>
