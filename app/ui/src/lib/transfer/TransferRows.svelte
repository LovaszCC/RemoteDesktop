<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Minus from '@lucide/svelte/icons/minus';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import type { TransferRowDto } from '../types';
  import type { ConflictChoice } from './model';

  interface Props {
    rows: readonly TransferRowDto[];
    included: ReadonlySet<string>;
    choices: Readonly<Record<string, ConflictChoice>>;
    /** Show the user and password columns (a CSV has neither). */
    logins?: boolean;
    oninclude: (ids: string[], on: boolean) => void;
    onchoice: (id: string, choice: ConflictChoice) => void;
  }

  /** The preview rows of the export and CSV imports: a checkbox each, a choice for existing ones. */
  let { rows, included, choices, logins = true, oninclude, onchoice }: Props = $props();

  const CHOICES: { value: Exclude<ConflictChoice, null>; label: string }[] = [
    { value: 'overwrite', label: 'Overwrite' },
    { value: 'new', label: 'Keep both' },
  ];
</script>

<div class="table" class:logins>
  <div class="row head" aria-hidden="true">
    <span></span>
    <span>{t('Name')}</span>
    <span>{t('Address')}</span>
    {#if logins}<span>{t('User')}</span>{/if}
    <span>{t('Group')}</span>
    {#if logins}<span class="center">{t('Password')}</span>{/if}
    <span></span>
  </div>
  {#each rows as row (row.id)}
    {@const on = included.has(row.id)}
    <div class="row" data-id={row.id}>
      <span>
        <input
          type="checkbox"
          checked={on}
          aria-label={row.name}
          onchange={(e) => oninclude([row.id], e.currentTarget.checked)}
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
      {#if logins}<span class="muted text">{row.user}</span>{/if}
      <span class="muted text">{row.folderPath.join(' / ')}</span>
      {#if logins}
        <span class="muted center" aria-label={row.hasPassword ? t('Password') : ''}>
          {#if row.hasPassword}<Check size={13} />{:else}<Minus size={13} />{/if}
        </span>
      {/if}
      <span>
        {#if row.conflict}
          <select
            class="choice"
            class:open={on && !choices[row.id]}
            disabled={!on}
            aria-label={t('already exists')}
            value={choices[row.id] ?? ''}
            onchange={(e) => onchoice(row.id, (e.currentTarget.value || null) as ConflictChoice)}
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

<style>
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
    grid-template-columns: 22px minmax(160px, 2fr) minmax(120px, 1.5fr) minmax(90px, 1.2fr) 120px;
    align-items: center;
    gap: 6px;
    padding: 3px 4px;
    border-radius: var(--radius-small);
  }
  .logins .row {
    grid-template-columns:
      22px minmax(140px, 2fr) minmax(90px, 1.3fr) minmax(60px, 1fr) minmax(70px, 1.1fr)
      50px 120px;
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
</style>
