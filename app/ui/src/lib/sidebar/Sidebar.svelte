<script lang="ts">
  import Search from '@lucide/svelte/icons/search';
  import { tick } from 'svelte';
  import { showContextMenu } from '../components/menu.svelte';
  import { t } from '../i18n/index.svelte';
  import { app } from '../stores/app.svelte';
  import { library as defaultLibrary, type LibraryStore } from '../stores/library.svelte';
  import { settings } from '../stores/settings.svelte';
  import { externalTerminalName, terminals } from '../stores/terminals.svelte';
  import type { Platform } from '../types';
  import { sidebarActions, type SidebarActions } from './actions.svelte';
  import { sidebarMenu } from './menus';
  import {
    DRAG_SPLIT,
    DRAG_TAGS,
    buildSections,
    dragPayload,
    emptyHint,
    isDraggable,
    isLibraryEmpty,
    menuTags,
    payloadTags,
    reorderedIds,
    stepSelection,
    terminalName,
    visibleOrder,
    type DropMark,
    type SidebarRow,
  } from './model';
  import LocalTerminalRow from './LocalTerminalRow.svelte';
  import SidebarRowView from './SidebarRowView.svelte';
  import { forwards as defaultForwards, type ForwardStore } from '../forwards/forwards.svelte';
  import { vpn as defaultVpn, type VpnStore } from '../vpn/vpn.svelte';
  import {
    connectionTabs as defaultConnectionTabs,
    type ConnectionTabs,
  } from '../terminal/connectionTabs.svelte';
  import { LOCAL_TERMINAL_ID } from '../terminal/local';

  interface Props {
    store?: LibraryStore;
    actions?: SidebarActions;
    /** Tunnel states for the port forward indicator. */
    forwards?: ForwardStore;
    /** Open embedded sessions, for the open session dot. */
    connectionTabs?: Pick<ConnectionTabs, 'isLive'>;
    /** Held VPNs, for the VPN badge and "Release VPN". */
    vpn?: Pick<VpnStore, 'holds' | 'externalProfile'>;
    readOnly?: boolean;
    platform?: Platform;
  }

  let {
    store = defaultLibrary,
    actions = sidebarActions,
    forwards = defaultForwards,
    connectionTabs = defaultConnectionTabs,
    vpn = defaultVpn,
    ...overrides
  }: Props = $props();
  const readOnly = $derived(overrides.readOnly ?? app.readOnly);
  const platform = $derived(overrides.platform ?? app.platform);

  const sections = $derived(
    buildSections({
      library: store.library,
      query: store.query,
      results: store.results,
      expanded: actions.expanded,
    }),
  );
  const order = $derived(visibleOrder(sections));
  const rowsByTag = $derived(new Map(sections.flatMap((s) => s.rows.map((r) => [r.tag, r] as const))));
  const empty = $derived(isLibraryEmpty(store.library, store.query));

  let list: HTMLDivElement | undefined = $state();

  // Drag state. WebKit hides drag data until the drop, so the dragged tags are kept here too.
  let dragTags = $state<string[] | null>(null);
  let dragSplit = $state<string | null>(null);
  /** `root`, a folder tag, or a split tag with its side. */
  let dropTarget = $state<{ tag: string; mark: Exclude<DropMark, null> } | null>(null);

  // Selection

  function click(row: SidebarRow, event: MouseEvent) {
    // Control-click is the secondary click on macOS: the context menu handles it.
    if (platform === 'macos' && event.ctrlKey) return;
    const toggleKey = platform === 'macos' ? event.metaKey : event.ctrlKey;
    if (event.shiftKey) store.extendTo(row.tag, order);
    else if (toggleKey) store.toggle(row.tag);
    else store.select(row.tag);
  }

  function activate(tag: string) {
    if (store.selection.length > 1) return;
    actions.primaryAction(tag);
  }

  async function reveal(tag: string) {
    await tick();
    list?.querySelector(`[data-tag="${CSS.escape(tag)}"]`)?.scrollIntoView?.({ block: 'nearest' });
  }

  function keydown(event: KeyboardEvent) {
    if (event.altKey || event.metaKey || event.ctrlKey) return;
    const current = store.primary;
    const row = current ? rowsByTag.get(current) : undefined;
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowUp': {
        const next = stepSelection(order, current, event.key === 'ArrowDown' ? 1 : -1);
        if (!next) return;
        if (event.shiftKey) store.extendTo(next, order);
        else store.select(next);
        void reveal(next);
        break;
      }
      case 'ArrowLeft':
      case 'ArrowRight': {
        if (row?.kind !== 'folder' && !(row?.kind === 'entry' && row.hasChildren)) return;
        const open = event.key === 'ArrowRight';
        if (row.expanded !== open) void actions.setExpanded(row.id, open);
        break;
      }
      case 'Enter':
        if (!store.single) return;
        actions.primaryAction(store.single);
        break;
      default:
        return;
    }
    event.preventDefault();
  }

  // Context menus

  function openMenu(event: MouseEvent, tag: string | null) {
    event.preventDefault();
    event.stopPropagation();
    const items = sidebarMenu(
      {
        library: store.library,
        readOnly,
        terminalName: externalTerminalName(
          settings.settings?.externalTerminal ?? null,
          terminals.installed,
          terminalName(platform),
        ),
        selectedFolderId: actions.selectedFolderId(),
        platform,
        externalVpn: (entryId) => vpn.externalProfile(entryId),
        actions,
      },
      menuTags(tag, store.selection),
    );
    showContextMenu(event, items);
  }

  // Drag and drop

  function dragStart(row: SidebarRow, event: DragEvent) {
    const data = event.dataTransfer;
    if (!data) return;
    data.effectAllowed = 'move';
    if (row.kind === 'split') {
      if (readOnly) return event.preventDefault();
      dragSplit = row.id;
      data.setData(DRAG_SPLIT, row.id);
      return;
    }
    if (!isDraggable(row) || readOnly) return event.preventDefault();
    const payload = dragPayload(row.tag, store.selection);
    dragTags = payloadTags(payload);
    data.setData(DRAG_TAGS, payload);
    data.setData('text/plain', payload);
  }

  function dragEnd() {
    dragTags = null;
    dragSplit = null;
    dropTarget = null;
  }

  function accept(event: DragEvent, tag: string, mark: Exclude<DropMark, null>) {
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = 'move';
    if (dropTarget?.tag !== tag || dropTarget.mark !== mark) dropTarget = { tag, mark };
  }

  function dragOver(row: SidebarRow, event: DragEvent) {
    if (dragTags && row.kind === 'folder') accept(event, row.tag, 'into');
    else if (dragSplit && row.kind === 'split') {
      const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
      accept(event, row.tag, event.clientY > rect.top + rect.height / 2 ? 'after' : 'before');
    }
  }

  function dragLeave(tag: string, event: DragEvent) {
    const into = event.relatedTarget as Node | null;
    if (into && (event.currentTarget as Node).contains(into)) return;
    if (dropTarget?.tag === tag) dropTarget = null;
  }

  function droppedTags(event: DragEvent): string[] {
    const payload = event.dataTransfer?.getData(DRAG_TAGS);
    return payload ? payloadTags(payload) : (dragTags ?? []);
  }

  function drop(row: SidebarRow | null, event: DragEvent) {
    event.preventDefault();
    const target = dropTarget;
    const tags = droppedTags(event);
    const split = dragSplit;
    dragEnd();
    if (row === null) {
      if (tags.length > 0) void actions.move(tags, null);
    } else if (row.kind === 'folder' && tags.length > 0) {
      void actions.move(tags, row.id);
      void actions.setExpanded(row.id, true);
    } else if (row.kind === 'split' && split && target) {
      const ids = store.library.savedSplits.map((s) => s.id);
      const next = reorderedIds(ids, split, row.id, target.mark === 'after');
      if (next.join() !== ids.join()) void actions.reorderSavedSplits(next);
    }
  }

  const markOf = (tag: string): DropMark => (dropTarget?.tag === tag ? dropTarget.mark : null);

  const hint = $derived(
    emptyHint([
      settings.shortcut('newRDP')?.display ?? null,
      settings.shortcut('newSSH')?.display ?? null,
      settings.shortcut('importRDM')?.display ?? null,
    ]),
  );
</script>

<div class="sidebar-search">
  <Search size={13} />
  <input
    type="search"
    placeholder={t('Search')}
    aria-label={t('Search')}
    value={store.query}
    oninput={(e) => store.setQuery(e.currentTarget.value)}
    onkeydown={(e) => {
      if (e.key === 'Escape' && store.query) {
        e.preventDefault();
        store.setQuery('');
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        list?.focus();
        keydown(e);
      }
    }}
  />
</div>

<div class="sidebar-fixed">
  <LocalTerminalRow {actions} sessionOpen={connectionTabs.isLive(LOCAL_TERMINAL_ID)} />
</div>

<div
  bind:this={list}
  class="sidebar-list"
  role="listbox"
  aria-multiselectable="true"
  aria-label={t('Connections')}
  tabindex="0"
  onkeydown={keydown}
  oncontextmenu={(e) => openMenu(e, null)}
>
  {#if empty}
    <div class="empty-state">
      <p class="empty-title">{t('No connections')}</p>
      <p class="empty-hint">{hint}</p>
    </div>
  {:else}
    {#each sections as section (section.id)}
      <div class="section" data-section={section.id}>
        {#if section.id === 'connections'}
          <h3
            class="header drop-zone"
            class:drop-into={dropTarget?.tag === 'root'}
            ondragover={(e) => dragTags && accept(e, 'root', 'into')}
            ondragleave={(e) => dragLeave('root', e)}
            ondrop={(e) => drop(null, e)}
          >
            {t(section.title)}
          </h3>
        {:else}
          <h3 class="header">{t(section.title)}</h3>
        {/if}
        {#each section.rows as row (row.tag)}
          {@const entry = row.kind === 'entry' ? store.entry(row.id) : undefined}
          {@const connection = store.connectionForTag(row.tag)}
          {@const connections = entry ? store.connectionsOf(entry.id) : connection ? [connection] : []}
          {@const split = row.kind === 'split' ? store.split(row.id) : undefined}
          <SidebarRowView
            {row}
            {entry}
            {connection}
            forwardRunning={connections.some((c) => c.type === 'ssh' && forwards.hasRunning(c.id))}
            sessionOpen={connections.some((c) => connectionTabs.isLive(c.id))}
            vpnHeld={platform === 'macos' && !!entry && vpn.holds(entry.id)}
            folder={row.kind === 'folder' ? store.folder(row.id) : undefined}
            credential={row.kind === 'credential' ? store.credential(row.id) : undefined}
            {split}
            splitNames={split ? split.connectionIds.flatMap((id) => store.connection(id)?.name ?? []) : []}
            selected={store.isSelected(row.tag)}
            drop={markOf(row.tag)}
            draggable={!readOnly && (isDraggable(row) || row.kind === 'split')}
            onpointer={(e) => click(row, e)}
            onactivate={() => activate(row.tag)}
            onmenu={(e) => openMenu(e, row.tag)}
            ontoggle={row.kind === 'folder' || row.kind === 'entry'
              ? () => void actions.setExpanded(row.id)
              : undefined}
            ondragstart={(e) => dragStart(row, e)}
            ondragover={(e) => dragOver(row, e)}
            ondragleave={(e) => dragLeave(row.tag, e)}
            ondrop={(e) => drop(row, e)}
            ondragend={dragEnd}
          />
        {/each}
      </div>
    {/each}
  {/if}
</div>

<style>
  .sidebar-search {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
    height: 28px;
    margin: 6px 10px 8px;
    padding: 0 9px;
    border-radius: 8px;
    background: var(--chrome-hover);
    box-shadow: inset 0 0 0 0.5px var(--separator);
    color: var(--fg-secondary);
  }
  .sidebar-search input {
    flex: 1;
    min-width: 0;
    border: none;
    background: none;
    color: var(--fg);
    outline: none;
  }
  .sidebar-fixed {
    flex: none;
    padding: 0 10px;
  }
  .sidebar-list {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 10px 10px;
    outline: none;
  }
  .section {
    margin-bottom: 6px;
  }
  .header {
    margin: 10px 0 2px;
    padding: 2px 4px;
    border-radius: var(--radius-small);
    color: var(--fg-tertiary);
    font-size: var(--font-size-small);
    font-weight: 600;
    letter-spacing: 0.01em;
  }
  .drop-into {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
    background: var(--accent-soft);
  }
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 100%;
    padding: var(--space-4);
    text-align: center;
  }
  .empty-title {
    margin: 0;
    color: var(--fg-secondary);
    font-size: var(--font-size-title);
    font-weight: 600;
  }
  .empty-hint {
    margin: 0;
    color: var(--fg-tertiary);
  }
</style>
