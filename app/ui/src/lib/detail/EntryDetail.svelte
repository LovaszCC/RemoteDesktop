<script lang="ts">
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Columns2 from '@lucide/svelte/icons/columns-2';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Merge from '@lucide/svelte/icons/merge';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Play from '@lucide/svelte/icons/play';
  import Star from '@lucide/svelte/icons/star';
  import UserRoundKey from '@lucide/svelte/icons/user-round-key';
  import Button from '../components/Button.svelte';
  import { showContextMenu } from '../components/menu.svelte';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import { usesSSH } from '../sidebar/model';
  import type { ConnectionDto, EntryDto, MikrotikMode } from '../types';
  import { viewDeps } from './deps';
  import DetailPage from './DetailPage.svelte';
  import FieldValue from './FieldValue.svelte';
  import PortForwardSection from '../forwards/PortForwardSection.svelte';
  import {
    canCopyPassword,
    connectMenuItems,
    connectionFields,
    connectionSubtitle,
    entryButtons,
    mikrotikModeName,
  } from './model';

  /**
   * An entry's Details (Swift `ConnectionDetailView`, per entry): a button per pinned connection (the
   * default primary), "Connect ▾" with every connection, the fields grouped per connection when there
   * are several, the open tabs, the port forwards and the notes. `selectedId` (a child row) highlights
   * that connection's group. A one-connection entry looks as a connection did before.
   */
  let {
    entry,
    connections,
    selectedId = null,
  }: { entry: EntryDto; connections: ConnectionDto[]; selectedId?: string | null } = $props();
  const deps = viewDeps();

  const primary = $derived(connections.find((c) => c.id === entry.defaultConnectionId) ?? connections[0]!);
  const single = $derived(connections.length === 1);
  const sshConnections = $derived(connections.filter((c) => c.type === 'ssh'));
  const buttons = $derived(entryButtons(entry, connections));
  /** Files: the default connection when it uses SSH, else the first that does. */
  const filesConnection = $derived(
    usesSSH(primary.type) ? primary : connections.find((c) => usesSSH(c.type)),
  );
  // Listed when the entry is open in more than one tab (with one, a click can go there directly).
  const openTabs = $derived(deps.connectionTabs.openTabs(entry.id));
  const MODES: MikrotikMode[] = ['winbox', 'ssh'];

  $effect(() => {
    if (connections.some((c) => c.type === 'web' && c.browserBundleId)) void deps.browsers.load();
  });

  async function copy(text: string) {
    try {
      await deps.api.copyText(text);
      deps.toast.show(t('Copied'));
    } catch (error) {
      deps.alerts.show(errorMessage(error));
    }
  }

  /** "Connect ▾", opened below the button. */
  function openConnectMenu(event: MouseEvent) {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    showContextMenu(
      { clientX: rect.left, clientY: rect.bottom + 2 },
      connectMenuItems(connections, entry.defaultConnectionId, (c) => void deps.actions.connect(c)),
    );
  }

  const subtitleOf = (connection: ConnectionDto) =>
    connectionSubtitle(connection, deps.platform, (id) => deps.browsers.name(id));
</script>

<DetailPage title={entry.name} subtitle={single ? subtitleOf(primary) : entry.host}>
  {#snippet icon()}
    <ConnectionIcon type={primary.type} size={30} />
  {/snippet}
  {#snippet accessory()}
    <button
      type="button"
      class="star"
      class:on={entry.isFavorite}
      aria-pressed={entry.isFavorite}
      aria-label={t('Favorite')}
      title={t('Favorite')}
      disabled={deps.readOnly}
      onclick={() => deps.actions.setFavorite(primary.id, !entry.isFavorite)}
    >
      <Star size={18} strokeWidth={1.75} fill={entry.isFavorite ? 'currentColor' : 'none'} />
    </button>
  {/snippet}

  <div class="actions">
    {#each buttons as button (button.connection.id)}
      {@const connection = button.connection}
      {#if connection.type === 'mikrotik'}
        <span class="group">
          {#each MODES as mode (mode)}
            <Button
              variant={button.primary && mode === connection.mikrotikMode ? 'primary' : 'default'}
              onclick={() => void deps.actions.connect(connection, { mode })}
            >
              <Play size={13} />{single
                ? mikrotikModeName(mode)
                : `${connection.displayLabel} · ${mikrotikModeName(mode)}`}
            </Button>
          {/each}
        </span>
      {:else}
        <Button
          variant={button.primary ? 'primary' : 'default'}
          onclick={() => void deps.actions.connect(connection)}
        >
          <Play size={13} />{single ? t('Connect') : connection.displayLabel}
        </Button>
      {/if}
    {/each}
    {#if !single}
      <Button aria-haspopup="menu" title={t('All connections')} onclick={openConnectMenu}>
        {t('Connect')}<ChevronDown size={13} />
      </Button>
    {/if}
    {#if filesConnection}
      {@const files = filesConnection}
      <Button
        title={t('Open the file browser (%@)', deps.settings.shortcut('openFiles')?.display ?? '')}
        onclick={() => void deps.actions.openFiles(files.id)}
      >
        <FolderOpen size={13} />{t('Files (SCP)…')}
      </Button>
    {/if}
    <Button
      disabled={deps.readOnly}
      onclick={() => deps.actions.editEntry(entry.id, selectedId ?? (single ? primary.id : undefined))}
    >
      <Pencil size={13} />{t('Edit')}
    </Button>
    {#if !deps.readOnly}
      <Button onclick={() => deps.actions.merge(entry.id)}>
        <Merge size={13} />{t('Merge…')}
      </Button>
    {/if}
    {#if canCopyPassword(primary)}
      <Button onclick={() => deps.actions.copyPassword(primary.id)}>
        <KeyRound size={13} />{t('Copy password')}
      </Button>
    {/if}
  </div>

  <!-- Several connections sit side by side in one scrollable row, so what follows (port forwards, notes)
       stays in view. -->
  <div class="connection-groups" class:row={!single}>
    {#each connections as connection (connection.id)}
      {@const fields = connectionFields(connection)}
      {@const credential = deps.library.credential(connection.credentialId)}
      <div
        class="connection-group"
        class:grouped={!single}
        class:selected={!single && connection.id === selectedId}
      >
        {#if !single}
          <div class="group-head">
            <h3>
              <ConnectionIcon type={connection.type} size={13} />
              <span>{connection.displayLabel}</span>
              {#if connection.id === entry.defaultConnectionId}
                <span class="default" title={t('Default connection')} aria-label={t('Default connection')}>
                  <CircleCheck size={12} strokeWidth={2} />
                </span>
              {/if}
            </h3>
            <span class="caption">{subtitleOf(connection)}</span>
          </div>
        {/if}
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
      </div>
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

  {#if sshConnections.length > 0}
    <PortForwardSection
      connections={sshConnections}
      defaultConnectionId={sshConnections.find((c) => c.id === entry.defaultConnectionId)?.id ??
        sshConnections[0]!.id}
    />
  {/if}

  {#if entry.notes}
    <section>
      <h2>{t('Notes')}</h2>
      <p class="notes selectable">{entry.notes}</p>
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
  .star.on {
    color: var(--label-yellow);
  }
  .star:disabled {
    opacity: 0.5;
  }
  .group {
    display: inline-flex;
    gap: var(--space-1);
  }
  .open-tabs {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
  }
  .connection-group {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .connection-group.grouped {
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius);
  }
  .connection-groups {
    display: flex;
    /* A scrolling row in the page's flex column would otherwise shrink and clip the cards. */
    flex-shrink: 0;
    flex-direction: column;
    gap: var(--space-3);
  }
  .connection-groups.row {
    flex-direction: row;
    /* Every card as tall as the tallest one. */
    align-items: stretch;
    overflow-x: auto;
    padding-bottom: var(--space-1);
  }
  .connection-groups.row .connection-group {
    flex: 0 0 auto;
    min-width: 260px;
    border: 0.5px solid var(--separator);
  }
  .connection-group.selected {
    background: var(--bg-selected);
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: var(--font-size);
    font-weight: 600;
  }
  .default {
    display: inline-flex;
    color: var(--accent);
  }
  .group-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
</style>
