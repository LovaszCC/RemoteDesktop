<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Minus from '@lucide/svelte/icons/minus';
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import SecureField from '../components/SecureField.svelte';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import type { ConflictChoice } from './model';
  import type { TransferImportStore } from './transferImporter.svelte';

  /** Import of an export: the password of an encrypted file, then the rows; existing ones need a choice. */
  let { importer }: { importer: TransferImportStore } = $props();

  const step = $derived(importer.passwordStep);
  const draft = $derived(importer.draft);
  const preview = $derived(draft?.preview);
  const conflicts = $derived(preview?.rows.filter((r) => r.conflict).length ?? 0);
  const allIds = $derived(preview?.rows.map((r) => r.id) ?? []);
  let password = $state('');

  const CHOICES: { value: Exclude<ConflictChoice, null>; label: string }[] = [
    { value: 'overwrite', label: 'Overwrite' },
    { value: 'new', label: 'Keep both' },
  ];

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
      <div class="table">
        <div class="row head" aria-hidden="true">
          <span></span>
          <span>{t('Name')}</span>
          <span>{t('Address')}</span>
          <span>{t('User')}</span>
          <span>{t('Group')}</span>
          <span class="center">{t('Password')}</span>
          <span></span>
        </div>
        {#each preview.rows as row (row.id)}
          {@const included = draft.included.has(row.id)}
          <div class="row" data-id={row.id}>
            <span>
              <input
                type="checkbox"
                checked={included}
                aria-label={row.name}
                onchange={(e) => importer.setIncluded([row.id], e.currentTarget.checked)}
              />
            </span>
            <span class="name">
              {#if row.type === 'credential'}
                <span class="credential-icon"><KeyRound size={14} strokeWidth={1.75} /></span>
              {:else}
                {#each row.connectionTypes as type (type)}<ConnectionIcon {type} size={13} />{/each}
              {/if}
              <span class="text">{row.name}</span>
            </span>
            <span class="muted text">{row.host}</span>
            <span class="muted text">{row.user}</span>
            <span class="muted text">{row.folderPath.join(' / ')}</span>
            <span class="muted center" aria-label={row.hasPassword ? t('Password') : ''}>
              {#if row.hasPassword}<Check size={13} />{:else}<Minus size={13} />{/if}
            </span>
            <span>
              {#if row.conflict}
                <select
                  class="choice"
                  class:open={included && !draft.choices[row.id]}
                  disabled={!included}
                  aria-label={t('already exists')}
                  value={draft.choices[row.id] ?? ''}
                  onchange={(e) =>
                    importer.setChoice(row.id, (e.currentTarget.value || null) as ConflictChoice)}
                >
                  <option value="" disabled>{t('already exists')} …</option>
                  {#each CHOICES as choice (choice.value)}
                    <option value={choice.value}>{t(choice.label)}</option>
                  {/each}
                </select>
              {/if}
            </span>
          </div>
        {/each}
      </div>
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
      22px minmax(140px, 2fr) minmax(90px, 1.3fr) minmax(60px, 1fr) minmax(70px, 1.1fr)
      50px 120px;
    align-items: center;
    gap: 6px;
    padding: 3px 4px;
    border-radius: var(--radius-small);
  }
  .row:not(.head):hover {
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
    gap: 4px;
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
  .choice {
    width: 100%;
    height: 22px;
    font-size: var(--font-size-small);
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-small);
    background: var(--bg-control);
  }
  .choice.open {
    border-color: var(--danger);
  }
  .choice:disabled {
    opacity: 0.5;
  }
  .spacer {
    flex: 1;
  }
</style>
