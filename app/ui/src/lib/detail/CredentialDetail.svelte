<script lang="ts">
  import CircleMinus from '@lucide/svelte/icons/circle-minus';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Plus from '@lucide/svelte/icons/plus';
  import Trash from '@lucide/svelte/icons/trash';
  import UserRoundKey from '@lucide/svelte/icons/user-round-key';
  import Button from '../components/Button.svelte';
  import Checkbox from '../components/Checkbox.svelte';
  import TextField from '../components/TextField.svelte';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import type { ConnectionDto, CredentialDto } from '../types';
  import { viewDeps } from './deps';
  import DetailPage from './DetailPage.svelte';
  import FieldValue from './FieldValue.svelte';
  import LinkSheet from './LinkSheet.svelte';
  import {
    NO_PICKS,
    allPicked,
    credentialFields,
    pickedAmong,
    setAllPicked,
    setPicked,
    usersOf,
  } from './model';

  /** Swift `CredentialDetailView`: the fields, "Used by" with search, select all and unlink, the link sheet. */
  let { credential }: { credential: CredentialDto } = $props();
  const deps = viewDeps();

  let query = $state('');
  let picked = $state<ReadonlySet<string>>(NO_PICKS);
  let linking = $state(false);
  /** The `matching` answer for the current query; `null` until it arrives. */
  let matched = $state<string[] | null>(null);
  let seq = 0;

  const users = $derived(usersOf(deps.library.library, credential.id));
  const userIds = $derived(users.map((c) => c.id));
  const shown = $derived(
    (matched ?? userIds)
      .map((id) => deps.library.connection(id))
      .filter((c): c is ConnectionDto => c !== undefined && c.credentialId === credential.id),
  );
  const shownIds = $derived(shown.map((c) => c.id));
  const toUnlink = $derived(pickedAmong(picked, userIds));

  $effect(() => {
    const ids = userIds;
    const q = query;
    const mine = ++seq;
    deps.api.matching(ids, q).then(
      (result) => {
        if (mine === seq) matched = result;
      },
      () => {
        if (mine === seq) matched = q.trim() ? [] : ids;
      },
    );
  });

  async function unlink() {
    const ids = toUnlink;
    await deps.actions.setCredential(
      null,
      ids.map((id) => `c:${id}`),
    );
    picked = setAllPicked(picked, ids, false);
  }
</script>

<DetailPage title={credential.name} subtitle={t('Credential')}>
  {#snippet icon()}
    <span class="credential-icon"><UserRoundKey size={30} strokeWidth={1.5} /></span>
  {/snippet}

  <div class="actions">
    <Button disabled={deps.readOnly} onclick={() => deps.actions.editCredential(credential.id)}>
      <Pencil size={13} />{t('Edit')}
    </Button>
    <Button
      variant="destructive"
      disabled={deps.readOnly}
      onclick={() => deps.actions.deleteCredential(credential.id)}
    >
      <Trash size={13} />{t('Delete')}
    </Button>
  </div>

  <div class="grid">
    {#each credentialFields(credential) as field (field.label)}
      <FieldValue {field} oncopy={() => {}} />
    {/each}
  </div>

  {#if credential.notes}
    <section>
      <h2>{t('Notes')}</h2>
      <p class="notes selectable">{credential.notes}</p>
    </section>
  {/if}

  <section class="used-by">
    <div class="section-head">
      <h2>{t('Used by (%lld)', users.length)}</h2>
      <Button
        disabled={deps.readOnly || toUnlink.length === 0}
        title={t('Unlink — the connection uses its own login')}
        onclick={unlink}
      >
        <CircleMinus size={13} />{t('Unlink %lld', toUnlink.length)}
      </Button>
      <Button disabled={deps.readOnly} onclick={() => (linking = true)}>
        <Plus size={13} />{t('Add connections…')}
      </Button>
    </div>
    {#if users.length === 0}
      <p class="muted">{t('No connections')}</p>
    {:else}
      <div class="filter">
        <TextField
          bind:value={query}
          placeholder={t('Name, host, user or folder')}
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
        {#each shown as connection (connection.id)}
          <li>
            <input
              type="checkbox"
              aria-label={connection.name}
              checked={picked.has(connection.id)}
              onchange={(e) => (picked = setPicked(picked, connection.id, e.currentTarget.checked))}
            />
            <button type="button" class="row" onclick={() => deps.library.selectConnection(connection.id)}>
              <ConnectionIcon type={connection.type} />
              <span>{connection.name}</span>
              <span class="muted">{connection.folderPath}</span>
            </button>
          </li>
        {/each}
      </ul>
      {#if shown.length === 0}
        <p class="muted">{t('No matches')}</p>
      {/if}
    {/if}
  </section>
</DetailPage>

{#if linking}
  <LinkSheet {credential} onclose={() => (linking = false)} />
{/if}

<style>
  .credential-icon {
    display: inline-flex;
    color: var(--label-orange);
  }
  .filter {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .filter :global(.search) {
    max-width: 320px;
  }
  ul {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  li input {
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--accent);
  }
  .row {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 2px 4px;
    border: none;
    border-radius: var(--radius-small);
    background: none;
    text-align: left;
  }
  .row:hover {
    background: var(--bg-hover);
  }
</style>
