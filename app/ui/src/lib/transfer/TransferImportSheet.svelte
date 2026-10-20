<script lang="ts">
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import SecureField from '../components/SecureField.svelte';
  import { t } from '../i18n/index.svelte';
  import type { TransferImportStore } from './transferImporter.svelte';
  import TransferRows from './TransferRows.svelte';

  /** Import of an export: the password of an encrypted file, then the rows; existing ones need a choice. */
  let { importer }: { importer: TransferImportStore } = $props();

  const step = $derived(importer.passwordStep);
  const draft = $derived(importer.draft);
  const preview = $derived(draft?.preview);
  const conflicts = $derived(preview?.rows.filter((r) => r.conflict).length ?? 0);
  const allIds = $derived(preview?.rows.map((r) => r.id) ?? []);
  let password = $state('');

  function unlock() {
    void importer.unlock(password);
  }
</script>

{#if step}
  <Modal title={t('Import from RemoteDesktop export')} onclose={() => importer.close()}>
    <div class="password-step">
      <p>{t('%@ is encrypted. Enter the password it was exported with.', step.fileName)}</p>
      <SecureField
        bind:value={password}
        onsubmit={unlock}
        invalid={step.wrong}
        aria-label={t('Password')}
        autocomplete="off"
      />
      {#if step.wrong}<p class="warning">{t('Wrong password')}</p>{/if}
    </div>
    {#snippet footer()}
      <Button onclick={() => importer.close()}>{t('Cancel')}</Button>
      <Button variant="primary" disabled={password === '' || importer.busy} onclick={unlock}>
        {t('Open')}
      </Button>
    {/snippet}
  </Modal>
{:else if draft && preview}
  <Modal title={t('Import from RemoteDesktop export')} onclose={() => importer.close()} width={760}>
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
      {#if !preview.encrypted}
        <p class="note">{t('This export has no passwords; enter them after the import.')}</p>
      {/if}
      <TransferRows
        rows={preview.rows}
        included={draft.included}
        choices={draft.choices}
        oninclude={(ids, on) => importer.setIncluded(ids, on)}
        onchoice={(id, choice) => importer.setChoice(id, choice)}
      />
    </div>
    {#snippet footer()}
      <Button onclick={() => importer.setIncluded(allIds, true)}>{t('Select all')}</Button>
      <Button onclick={() => importer.setIncluded(allIds, false)}>{t('Select none')}</Button>
      <span class="spacer"></span>
      <Button onclick={() => importer.close()}>{t('Cancel')}</Button>
      <Button
        variant="primary"
        disabled={importer.includedCount === 0 || importer.unanswered > 0 || importer.busy}
        onclick={() => void importer.commit()}
      >
        {t('Import %lld', importer.includedCount)}
      </Button>
    {/snippet}
  </Modal>
{/if}

<style>
  .password-step {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .warning {
    color: var(--danger);
    font-size: var(--font-size-small);
  }
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
  .spacer {
    flex: 1;
  }
</style>
