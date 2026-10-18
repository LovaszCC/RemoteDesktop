<script lang="ts">
  import Download from '@lucide/svelte/icons/download';
  import Lock from '@lucide/svelte/icons/lock';
  import PanelLeft from '@lucide/svelte/icons/panel-left';
  import Plus from '@lucide/svelte/icons/plus';
  import Settings from '@lucide/svelte/icons/settings';
  import { settingsWindow } from '../stores/settingsWindow.svelte';
  import AppShell from '../components/AppShell.svelte';
  import Banner from '../components/Banner.svelte';
  import DialogHost from '../components/DialogHost.svelte';
  import IconButton from '../components/IconButton.svelte';
  import { showContextMenu } from '../components/menu.svelte';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import DetailView from '../detail/DetailView.svelte';
  import EditorHost from '../editors/EditorHost.svelte';
  import { sidebarActions } from '../sidebar/actions.svelte';
  import ImportSheet from '../importer/ImportSheet.svelte';
  import { importer } from '../importer/importer.svelte';
  import { importMenu, newMenu } from '../sidebar/menus';
  import Sidebar from '../sidebar/Sidebar.svelte';
  import { alerts } from '../stores/alert.svelte';
  import { app } from '../stores/app.svelte';
  import { dialogs } from '../stores/dialogs.svelte';
  import { editor } from '../stores/editor.svelte';
  import { library } from '../stores/library.svelte';
  import { settings } from '../stores/settings.svelte';
  import { sidebarLayout } from '../stores/sidebarLayout.svelte';
  import RdpSessionView from '../rdp/RdpSessionView.svelte';
  import { rdpSessions } from '../rdp/sessions.svelte';
  import { tabs } from '../tabs.svelte';
  import { sessions } from '../terminal/sessions.svelte';
  import TerminalSessionView from '../terminal/TerminalSessionView.svelte';
  import SplitView from '../terminal/SplitView.svelte';
  import { splits } from '../terminal/splits.svelte';
  import TabBar from './TabBar.svelte';
  import { onDestroy, untrack } from 'svelte';

  // A sidebar selection change shows the Details tab (Swift `selection.didSet`).
  $effect(() => {
    void library.selection;
    untrack(() => tabs.select(null));
  });

  // Automatic tab colours follow the connections' colours.
  $effect(() => {
    const connections = library.library.connections;
    untrack(() => {
      sessions.syncConnections(connections);
      rdpSessions.syncConnections(connections);
      splits.syncConnections();
    });
  });

  const sidebarToggleLabel = $derived.by(() => {
    const hint = settings.shortcut('toggleSidebar')?.display;
    const hidden = sidebarLayout.collapsed;
    if (!hint) return t(hidden ? 'Show sidebar' : 'Hide sidebar');
    return t(hidden ? 'Show sidebar (%@)' : 'Hide sidebar (%@)', hint);
  });
  const activeSession = $derived(tabs.activeSession);
  const activeIsTerminal = $derived(
    activeSession !== null &&
      (sessions.has(activeSession.id) || splits.has(activeSession.id) || rdpSessions.has(activeSession.id)),
  );
  const activeColor = $derived(activeSession ? tabs.displayColor(activeSession) : null);

  // Locking leaves the main view: open sheets don't come back after the next unlock.
  onDestroy(() => {
    editor.closeAll();
    dialogs.cancel();
    importer.close();
  });

  /** The toolbar "+" menu, opened below the button. */
  function openNewMenu(event: MouseEvent) {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    showContextMenu(
      { clientX: rect.left, clientY: rect.bottom + 2 },
      newMenu({ readOnly: app.readOnly, actions: sidebarActions }, sidebarActions.selectedFolderId()),
    );
  }

  /** The toolbar Import menu, opened below the button. */
  function openImportMenu(event: MouseEvent) {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    showContextMenu(
      { clientX: rect.left, clientY: rect.bottom + 2 },
      importMenu(app.readOnly, (kind) => void importer.start(kind), {
        rdm: settings.shortcut('importRDM')?.display ?? null,
        winbox: settings.shortcut('importWinbox')?.display ?? null,
      }),
    );
  }

  async function lock() {
    try {
      await app.lock();
    } catch (error) {
      alerts.show(errorMessage(error));
    }
  }
</script>

<AppShell>
  {#snippet sidebar()}
    <Sidebar />
  {/snippet}

  {#snippet toolbar()}
    <IconButton
      icon={PanelLeft}
      label={sidebarToggleLabel}
      active={!sidebarLayout.collapsed}
      onclick={() => sidebarLayout.toggle()}
    />
    <IconButton
      icon={Plus}
      label={t('New')}
      title={t('New connection or folder')}
      disabled={app.readOnly}
      aria-haspopup="menu"
      onclick={openNewMenu}
    />
    <IconButton
      icon={Download}
      label={t('Import')}
      title={t('Import from a Devolutions RDM export (.rdm) or a WinBox address book (.cdb)')}
      disabled={app.readOnly}
      aria-haspopup="menu"
      onclick={openImportMenu}
    />
    <span class="spacer"></span>
    <IconButton icon={Settings} label={t('Settings…')} onclick={() => settingsWindow.open()} />
    <IconButton icon={Lock} label={t('Lock')} onclick={lock} />
  {/snippet}

  {#snippet header()}
    {#if app.readOnly}
      <Banner
        kind="warning"
        message={t('The database is open read-only because another computer is using it.')}
      />
    {/if}
    {#if tabs.hasSessions}<TabBar />{/if}
    {#if activeColor}<div class="session-color" style:background="var(--label-{activeColor})"></div>{/if}
  {/snippet}

  <!-- Terminals and RDP views stay mounted while hidden so nothing is re-laid out on every tab switch. -->
  {#each tabs.sessions as session (session.id)}
    {#if splits.has(session.id)}
      <div class="terminal" class:hidden={tabs.active !== session.id}>
        <SplitView {splits} {sessions} id={session.id} active={tabs.active === session.id} />
      </div>
    {:else if sessions.has(session.id)}
      <div class="terminal" class:hidden={tabs.active !== session.id}>
        <TerminalSessionView {sessions} id={session.id} active={tabs.active === session.id} />
      </div>
    {:else if rdpSessions.has(session.id)}
      <div class="terminal" class:hidden={tabs.active !== session.id}>
        <RdpSessionView sessions={rdpSessions} id={session.id} active={tabs.active === session.id} />
      </div>
    {/if}
  {/each}
  {#if activeIsTerminal}
    <!-- shown above -->
  {:else if activeSession}
    <div class="session-placeholder">
      <p class="session-title">{tabs.displayTitle(activeSession)}</p>
      <p>{t('Sessions are not available yet')}</p>
    </div>
  {:else}
    <DetailView />
  {/if}
</AppShell>

<EditorHost />
<DialogHost />
<ImportSheet {importer} />

<style>
  .spacer {
    flex: 1;
  }
  .session-color {
    flex: none;
    height: 3px;
  }
  .terminal {
    height: 100%;
  }
  .terminal.hidden {
    display: none;
  }
  .session-placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--fg-secondary);
  }
  .session-title {
    color: var(--fg);
    font-weight: 600;
  }
</style>
