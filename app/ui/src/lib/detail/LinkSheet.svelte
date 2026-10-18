<script lang="ts">
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import TextField from '../components/TextField.svelte';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import type { ConnectionDto, CredentialDto } from '../types';
  import { viewDeps } from './deps';
  import { NO_PICKS, setPicked } from './model';

  /**
   * Swift `LinkConnectionsSheet`: picks connections to link to a credential; ones linked to another
   * credential move over. Picks stay when the search hides them.
   */
  let { credential, onclose }: { credential: CredentialDto; onclose: () => void } = $props();
  const deps = viewDeps();

  let query = $state('');
  let picked = $state<ReadonlySet<string>>(NO_PICKS);
  let candidateIds = $state<string[]>([]);
  let seq = 0;

  const candidates = $derived(
    candidateIds.map((id) => deps.library.connection(id)).filter((c): c is ConnectionDto => c !== undefined),
  );

  $effect(() => {
    const q = query;
    const id = credential.id;
    // Re-run when the library changes too.
    void deps.library.library;
    const mine = ++seq;
    deps.api.linkCandidates(id, q).then(
      (ids) => {
        if (mine === seq) candidateIds = ids;
      },
      () => {
        if (mine === seq) candidateIds = [];
      },
    );
  });

  async function add() {
    const ids = [...picked];
    onclose();
    await deps.actions.setCredential(
      credential.id,
      ids.map((id) => `c:${id}`),
    );
  }
</script>

<Modal title={t('Link connections to “%@”', credential.name)} {onclose} width={560}>
  <div class="link-sheet">
    <TextField bind:value={query} placeholder={t('Name, host, user or folder')} aria-label={t('Search')} />
    <ul>
      {#each candidates as connection (connection.id)}
        {@const current = deps.library.credential(connection.credentialId)}
        <li>
          <label>
            <input
              type="checkbox"
              checked={picked.has(connection.id)}
              onchange={(e) => (picked = setPicked(picked, connection.id, e.currentTarget.checked))}
            />
            <ConnectionIcon type={connection.type} />
            <span>{connection.name}</span>
            <span class="muted">{connection.folderPath}</span>
            {#if current}
              <span class="capsule">{t('currently: %@ — will move', current.name)}</span>
            {/if}
          </label>
        </li>
      {:else}
        <li class="empty">{t('No connections')}</li>
      {/each}
    </ul>
  </div>
  {#snippet footer()}
    <Button onclick={onclose}>{t('Cancel')}</Button>
    <Button variant="primary" disabled={picked.size === 0} onclick={add}>{t('Add %lld', picked.size)}</Button>
  {/snippet}
</Modal>

<style>
  .link-sheet {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    height: 340px;
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
    color: var(--fg-secondary);
  }
  .capsule {
    padding: 0 6px;
    border-radius: 999px;
    background: var(--bg-selected);
    font-size: var(--font-size-small);
  }
  .empty {
    padding: var(--space-5);
    color: var(--fg-secondary);
    text-align: center;
  }
</style>
