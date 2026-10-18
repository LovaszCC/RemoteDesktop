<script lang="ts">
  // One side of the Files window (Swift `RemotePane` / `LocalPane`): a path bar with Up, a table with
  // sortable Name / Size / Modified / Permissions columns, click / ⌘-click / ⇧-click selection, double
  // click (or Enter) to open a folder, and a pointer-down hook the window uses to drag rows to the other
  // pane.
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import ChevronUp from '@lucide/svelte/icons/chevron-up';
  import FileIcon from '@lucide/svelte/icons/file';
  import Folder from '@lucide/svelte/icons/folder';
  import Link from '@lucide/svelte/icons/link';
  import IconButton from '../components/IconButton.svelte';
  import { i18n, t } from '../i18n/index.svelte';
  import type { FileEntryDto } from '../types';
  import type { FilesStore, Side } from './files.svelte';
  import { clickSelection, formatDate, formatSize, nextSort, type SortKey } from './model';

  interface Props {
    store: FilesStore;
    side: Side;
    title: string;
    /** Shown as a drop target (a drag from the other pane or the OS is over it). */
    highlight?: boolean;
    onrowpointerdown?: (event: PointerEvent, entry: FileEntryDto) => void;
  }

  let { store, side, title, highlight = false, onrowpointerdown }: Props = $props();

  const pane = $derived(store.pane(side));
  const rows = $derived(store.rows(side));
  const names = $derived(rows.map((r) => r.name));
  const isMac = typeof navigator !== 'undefined' && /Mac/.test(navigator.userAgent);
  // The typed path; follows the listing until edited.
  let pathText = $derived(pane.listing?.path ?? '');

  const COLUMNS: { key: SortKey; label: string }[] = [
    { key: 'name', label: 'Name' },
    { key: 'size', label: 'Size' },
    { key: 'modified', label: 'Modified' },
    { key: 'permissions', label: 'Permissions' },
  ];

  function click(event: Pick<MouseEvent, 'metaKey' | 'ctrlKey' | 'shiftKey'>, entry: FileEntryDto) {
    const next = clickSelection(pane.selection, names, entry.name, pane.anchor, {
      toggle: isMac ? event.metaKey : event.ctrlKey,
      range: event.shiftKey,
    });
    store.setSelection(side, next.selection, next.anchor);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === 'a' && (isMac ? event.metaKey : event.ctrlKey)) {
      event.preventDefault();
      store.setSelection(side, [...names], names[0] ?? null);
      return;
    }
    if (event.key === 'Enter') {
      const entry = rows.find((r) => r.name === pane.selection[0]);
      if (entry && pane.selection.length === 1) void store.open(side, entry);
      return;
    }
    if (event.key === 'Backspace' && (isMac ? event.metaKey : event.altKey)) {
      event.preventDefault();
      void store.up(side);
      return;
    }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    event.preventDefault();
    if (names.length === 0) return;
    const current = pane.anchor ? names.indexOf(pane.anchor) : -1;
    const step = event.key === 'ArrowDown' ? 1 : -1;
    const index = Math.min(Math.max(current + step, 0), names.length - 1);
    const name = names[index] ?? null;
    store.setSelection(side, name ? [name] : [], name);
  }

  function go(event: KeyboardEvent) {
    if (event.key === 'Enter' && pathText.trim()) void store.list(side, pathText.trim());
    if (event.key === 'Escape') pathText = pane.listing?.path ?? '';
  }
</script>

<section class="pane" class:highlight data-pane={side} aria-label={title}>
  <header>
    <span class="title">{title}</span>
    <IconButton
      icon={ArrowUp}
      label={t('Parent folder')}
      disabled={!pane.listing?.parent}
      onclick={() => void store.up(side)}
    />
    <input
      class="path selectable"
      aria-label={t('Path')}
      spellcheck="false"
      bind:value={pathText}
      onkeydown={go}
    />
  </header>
  <div class="table" role="grid" tabindex="0" aria-multiselectable="true" onkeydown={keydown}>
    <div class="row head" role="row">
      {#each COLUMNS as column (column.key)}
        <button
          type="button"
          role="columnheader"
          class="cell {column.key}"
          aria-sort={pane.sort.key === column.key
            ? pane.sort.ascending
              ? 'ascending'
              : 'descending'
            : 'none'}
          onclick={() => store.setSort(side, nextSort(pane.sort, column.key))}
        >
          {t(column.label)}
          {#if pane.sort.key === column.key}
            {#if pane.sort.ascending}<ChevronUp size={11} />{:else}<ChevronDown size={11} />{/if}
          {/if}
        </button>
      {/each}
    </div>
    <div class="body">
      {#each rows as entry (entry.name)}
        <div
          class="row"
          role="row"
          aria-selected={pane.selection.includes(entry.name)}
          class:selected={pane.selection.includes(entry.name)}
          data-name={entry.name}
          tabindex="-1"
          onclick={(e) => click(e, entry)}
          onkeydown={(e) => {
            if (e.key === ' ') click(e, entry);
          }}
          ondblclick={() => void store.open(side, entry)}
          onpointerdown={(e) => onrowpointerdown?.(e, entry)}
        >
          <span class="cell name" role="gridcell">
            {#if entry.kind === 'directory'}
              <Folder size={14} class="icon folder" />
            {:else if entry.kind === 'symlink'}
              <Link size={14} class="icon" />
            {:else}
              <FileIcon size={14} class="icon" />
            {/if}
            <span class="text">{entry.name}</span>
          </span>
          <span class="cell size" role="gridcell">
            {entry.kind === 'directory' ? '—' : formatSize(entry.size, i18n.locale)}
          </span>
          <span class="cell modified" role="gridcell">{formatDate(entry.modified, i18n.locale)}</span>
          <span class="cell permissions" role="gridcell">{entry.permissions}</span>
        </div>
      {:else}
        {#if pane.listing && !pane.loading}
          <p class="empty">{t('Empty folder')}</p>
        {/if}
      {/each}
    </div>
    {#if pane.loading}<div class="loading" aria-hidden="true"></div>{/if}
  </div>
  {#if pane.error}<p class="error" role="alert">{pane.error}</p>{/if}
</section>

<style>
  .pane {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    border: 2px solid transparent;
    border-radius: var(--radius);
  }
  .highlight {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-2);
  }
  .title {
    font-weight: 600;
    white-space: nowrap;
  }
  .path {
    flex: 1;
    min-width: 0;
    height: var(--control-height);
    padding: 0 6px;
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-small);
    background: var(--bg-control);
    color: var(--fg);
    font: inherit;
  }
  .table {
    position: relative;
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    outline: none;
  }
  .table:focus-visible {
    box-shadow: inset 0 0 0 2px var(--focus-ring);
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 80px 150px 92px;
    align-items: center;
    min-height: 22px;
    padding: 0 var(--space-2);
    user-select: none;
  }
  .head {
    border-bottom: 0.5px solid var(--separator);
    color: var(--fg-secondary);
    font-size: var(--font-size-callout);
  }
  .head .cell {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px 4px;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  .head .cell:hover {
    color: var(--fg);
  }
  .body {
    flex: 1;
    overflow: auto;
  }
  .body .row:nth-child(even) {
    background: var(--bg-hover);
  }
  .body .row.selected {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .cell {
    overflow: hidden;
    padding: 0 4px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .name .text {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .row :global(.icon) {
    flex: none;
    color: var(--fg-secondary);
  }
  .row :global(.folder) {
    color: var(--accent);
  }
  .row.selected :global(.icon) {
    color: inherit;
  }
  .size,
  .modified,
  .permissions {
    color: var(--fg-secondary);
  }
  .body .row.selected .cell {
    color: inherit;
  }
  .size {
    text-align: right;
  }
  .head .size {
    justify-content: flex-end;
  }
  .permissions {
    font-family: var(--font-mono);
    font-size: var(--font-size-small);
  }
  .empty {
    margin: var(--space-4);
    color: var(--fg-tertiary);
    text-align: center;
  }
  .loading {
    position: absolute;
    top: 0;
    right: 0;
    left: 0;
    height: 2px;
    background: linear-gradient(90deg, transparent, var(--accent), transparent);
    background-size: 50% 100%;
    animation: slide 1s linear infinite;
  }
  @keyframes slide {
    from {
      background-position: -50% 0;
    }
    to {
      background-position: 150% 0;
    }
  }
  .error {
    margin: 0;
    padding: 6px var(--space-2);
    color: var(--danger);
    font-size: var(--font-size-callout);
    overflow-wrap: anywhere;
  }
</style>
