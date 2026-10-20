<script lang="ts">
  import Folder from '@lucide/svelte/icons/folder';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import SecureField from '../components/SecureField.svelte';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import type { LibraryDto } from '../types';
  import type { ExportStore } from './exporter.svelte';
  import { exportGroups, groupState, linkedCredentials, missingCredentials } from './model';

  /** Export: entries by folder and credentials with checkboxes, an optional password. */
  let { exporter, library }: { exporter: ExportStore; library: LibraryDto } = $props();

  const draft = $derived(exporter.draft);
  const groups = $derived(exportGroups(library));
  const allEntries = $derived(library.entries.map((e) => e.id));
  const allCredentials = $derived(library.credentials.map((c) => c.id));
  const connectionTypes = $derived(
    new Map(
      library.entries.map((entry) => [
        entry.id,
        [
          ...new Set(
            entry.connectionIds
              .map((id) => library.connections.find((c) => c.id === id)?.type)
              .filter((type) => type !== undefined),
          ),
        ],
      ]),
    ),
  );
  const missing = $derived(
    draft ? new Set(missingCredentials(library, draft.entries, draft.credentials)) : new Set<string>(),
  );
  const credentialName = (id: string) => library.credentials.find((c) => c.id === id)?.name ?? '';

  /** Sets a checkbox's indeterminate state (a folder with only some of its entries checked). */
  function indeterminate(on: boolean) {
    return (node: HTMLInputElement) => {
      node.indeterminate = on;
    };
  }
</script>

{#if draft}
  <Modal title={t('Export')} onclose={() => exporter.close()} width={760}>
    <div class="export-sheet">
      <div class="lists">
        <section class="list">
          <h3>{t('Entries')} <span class="count">{draft.entries.size} / {allEntries.length}</span></h3>
          <div class="table">
            {#each groups as group (group.path)}
              {@const ids = group.entries.map((e) => e.id)}
              {@const state = groupState(draft.entries, ids)}
              <label class="row folder" data-folder={group.path}>
                <input
                  type="checkbox"
                  checked={state === 'all'}
                  {@attach indeterminate(state === 'some')}
                  onchange={(e) => exporter.setEntries(ids, e.currentTarget.checked)}
                />
                <span class="folder-icon"><Folder size={14} strokeWidth={1.75} /></span>
                <span class="text">{group.path || t('(Root)')}</span>
              </label>
              {#each group.entries as entry (entry.id)}
                {@const lost = linkedCredentials(library, entry.id).filter(
                  (id) => draft.entries.has(entry.id) && missing.has(id),
                )}
                <label class="row entry" data-id={entry.id}>
                  <input
                    type="checkbox"
                    checked={draft.entries.has(entry.id)}
                    onchange={(e) => exporter.setEntries([entry.id], e.currentTarget.checked)}
                  />
                  <span class="icons">
                    {#each connectionTypes.get(entry.id) ?? [] as type (type)}
                      <ConnectionIcon {type} size={13} />
                    {/each}
                  </span>
                  <span class="text">{entry.name}</span>
                  <span class="muted text">{entry.host}</span>
                  {#if lost.length > 0}
                    <span class="hint" title={lost.map(credentialName).join(', ')}>
                      {t('without credential')}
                    </span>
                  {/if}
                </label>
              {/each}
            {/each}
          </div>
        </section>
        <section class="list credentials">
          <h3>
            {t('Credentials')}
            <span class="count">{draft.credentials.size} / {allCredentials.length}</span>
          </h3>
          <div class="table">
            {#each library.credentials as credential (credential.id)}
              <label class="row" data-id={credential.id}>
                <input
                  type="checkbox"
                  checked={draft.credentials.has(credential.id)}
                  onchange={(e) => exporter.setCredentials([credential.id], e.currentTarget.checked)}
                />
                <span class="credential-icon"><KeyRound size={13} strokeWidth={1.75} /></span>
                <span class="text">{credential.name}</span>
                <span class="muted text">{credential.displayUser}</span>
              </label>
            {:else}
              <p class="empty">{t('No credentials')}</p>
            {/each}
          </div>
        </section>
      </div>
      <div class="password">
        <label>
          <span>{t('Password (optional)')}</span>
          <SecureField
            value={draft.password}
            oninput={(e) => exporter.setPassword(e.currentTarget.value)}
            autocomplete="new-password"
          />
        </label>
        <label>
          <span>{t('Confirm password')}</span>
          <SecureField
            value={draft.confirm}
            oninput={(e) => exporter.setConfirm(e.currentTarget.value)}
            invalid={exporter.problem === 'mismatch'}
            autocomplete="new-password"
          />
        </label>
      </div>
      <p class="note" class:warning={exporter.problem === 'mismatch'}>
        {#if exporter.problem === 'mismatch'}
          {t("The passwords don't match.")}
        {:else if draft.password}
          {t('The file is encrypted and contains the passwords. SSH keys are never exported.')}
        {:else}
          {t(
            'Without a password the file is readable and contains no passwords. SSH keys are never exported.',
          )}
        {/if}
      </p>
    </div>
    {#snippet footer()}
      <Button
        onclick={() => {
          exporter.setEntries(allEntries, true);
          exporter.setCredentials(allCredentials, true);
        }}>{t('Select all')}</Button
      >
      <Button
        onclick={() => {
          exporter.setEntries(allEntries, false);
          exporter.setCredentials(allCredentials, false);
        }}>{t('Select none')}</Button
      >
      <span class="spacer"></span>
      <Button onclick={() => exporter.close()}>{t('Cancel')}</Button>
      <Button
        variant="primary"
        disabled={exporter.problem !== null || exporter.busy}
        onclick={() => void exporter.commit()}
      >
        {t('Export…')}
      </Button>
    {/snippet}
  </Modal>
{/if}

<style>
  .export-sheet {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .lists {
    display: grid;
    grid-template-columns: 3fr 2fr;
    gap: var(--space-3);
    height: 340px;
  }
  .list {
    display: flex;
    flex-direction: column;
    min-height: 0;
    gap: 4px;
  }
  h3 {
    display: flex;
    justify-content: space-between;
    margin: 0;
    font-size: var(--font-size-small);
    font-weight: 600;
    color: var(--fg-secondary);
  }
  .count {
    font-weight: 400;
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
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 3px 4px;
    border-radius: var(--radius-small);
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .row.folder {
    margin-top: 2px;
    font-weight: 600;
  }
  .row.entry {
    padding-left: 22px;
  }
  .icons {
    display: inline-flex;
    flex: none;
    gap: 1px;
  }
  .folder-icon,
  .credential-icon {
    display: inline-flex;
    flex: none;
    color: var(--label-gray);
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted {
    flex: 1;
    color: var(--fg-secondary);
  }
  .hint {
    flex: none;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--bg-selected);
    font-size: var(--font-size-small);
  }
  .empty {
    margin: 4px;
    color: var(--fg-secondary);
  }
  input[type='checkbox'] {
    flex: none;
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--accent);
  }
  .password {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }
  .password label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .password span {
    font-size: var(--font-size-small);
    color: var(--fg-secondary);
  }
  .note {
    margin: 0;
    font-size: var(--font-size-small);
    color: var(--fg-secondary);
  }
  .note.warning {
    color: var(--danger);
  }
  .spacer {
    flex: 1;
  }
</style>
