<script lang="ts">
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Play from '@lucide/svelte/icons/play';
  import Columns2 from '@lucide/svelte/icons/columns-2';
  import Star from '@lucide/svelte/icons/star';
  import UserRoundKey from '@lucide/svelte/icons/user-round-key';
  import Button from '../components/Button.svelte';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import { usesSSH } from '../sidebar/model';
  import type { ConnectionDto, MikrotikMode } from '../types';
  import { viewDeps } from './deps';
  import DetailPage from './DetailPage.svelte';
  import FieldValue from './FieldValue.svelte';
  import PortForwardSection from '../forwards/PortForwardSection.svelte';
  import { canCopyPassword, connectionFields, connectionSubtitle, mikrotikModeName } from './model';

  /** Swift `ConnectionDetailView`. */
  let { connection }: { connection: ConnectionDto } = $props();
  const deps = viewDeps();

  const credential = $derived(deps.library.credential(connection.credentialId));
  const fields = $derived(connectionFields(connection));
  // Listed when the connection is open in more than one tab (with one, a click can go there directly).
  const openTabs = $derived(deps.connectionTabs.openTabs(connection.id));
  const MODES: MikrotikMode[] = ['winbox', 'ssh'];

  $effect(() => {
    if (connection.type === 'web' && connection.browserBundleId) void deps.browsers.load();
  });

  async function copy(text: string) {
    try {
      await deps.api.copyText(text);
      deps.toast.show(t('Copied'));
    } catch (error) {
      deps.alerts.show(errorMessage(error));
    }
  }
</script>

<DetailPage
  title={connection.name}
  subtitle={connectionSubtitle(connection, deps.platform, (id) => deps.browsers.name(id))}
>
  {#snippet icon()}
    <ConnectionIcon type={connection.type} size={30} />
  {/snippet}
  {#snippet accessory()}
    <button
      type="button"
      class="star"
      class:on={connection.isFavorite}
      aria-pressed={connection.isFavorite}
      aria-label={t('Favorite')}
      title={t('Favorite')}
      disabled={deps.readOnly}
      onclick={() => deps.actions.setFavorite(connection.id, !connection.isFavorite)}
    >
      <Star size={18} strokeWidth={1.75} fill={connection.isFavorite ? 'currentColor' : 'none'} />
    </button>
  {/snippet}

  <div class="actions">
    {#if connection.type === 'mikrotik'}
      {#each MODES as mode (mode)}
        <Button
          variant={mode === connection.mikrotikMode ? 'primary' : 'default'}
          onclick={() => void deps.actions.connect(connection, { mode })}
        >
          <Play size={13} />{mikrotikModeName(mode)}
        </Button>
      {/each}
    {:else}
      <Button variant="primary" onclick={() => void deps.actions.connect(connection)}>
        <Play size={13} />{t('Connect')}
      </Button>
    {/if}
    {#if usesSSH(connection.type)}
      <Button
        title={t('Open the file browser (%@)', deps.settings.shortcut('openFiles')?.display ?? '')}
        onclick={() => void deps.actions.openFiles(connection.id)}
      >
        <FolderOpen size={13} />{t('Files (SCP)…')}
      </Button>
    {/if}
    <Button disabled={deps.readOnly} onclick={() => deps.actions.editConnection(connection.id)}>
      <Pencil size={13} />{t('Edit')}
    </Button>
    {#if canCopyPassword(connection)}
      <Button onclick={() => deps.actions.copyPassword(connection.id)}>
        <KeyRound size={13} />{t('Copy password')}
      </Button>
    {/if}
  </div>

  <div class="grid">
    {#each fields.before as field (field.label)}
      <FieldValue {field} oncopy={copy} />
    {/each}
    {#if credential}
      <span class="label">{t('Credential')}</span>
      <span class="value">
        <Button variant="link" onclick={() => deps.library.select(`k:${credential.id}`)}>
          <UserRoundKey size={13} />{credential.name}
        </Button>
      </span>
    {/if}
    {#each fields.after as field (field.label)}
      <FieldValue {field} oncopy={copy} />
    {/each}
  </div>

  {#if openTabs.length > 1}
    <section>
      <h2>{t('Open tabs')}</h2>
      <div class="open-tabs">
        {#each openTabs as tab (tab.id)}
          <Button variant="link" onclick={() => deps.tabs.select(tab.id)}>
            {#if tab.split}<Columns2 size={13} />{:else}<ConnectionIcon
                type={tab.type}
                size={13}
              />{/if}{tab.title}
          </Button>
        {/each}
      </div>
    </section>
  {/if}

  {#if connection.type === 'ssh'}
    <PortForwardSection {connection} />
  {/if}

  {#if connection.notes}
    <section>
      <h2>{t('Notes')}</h2>
      <p class="notes selectable">{connection.notes}</p>
    </section>
  {/if}
</DetailPage>

<style>
  .star {
    display: inline-flex;
    padding: 4px;
    border: none;
    border-radius: var(--radius-small);
    background: none;
    color: var(--fg-secondary);
  }
  .star:hover:not(:disabled) {
    background: var(--bg-hover);
  }
  .open-tabs {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
  }
  .star.on {
    color: var(--label-yellow);
  }
  .star:disabled {
    opacity: 0.5;
  }
</style>
