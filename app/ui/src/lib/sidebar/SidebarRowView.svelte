<script lang="ts">
  import ArrowLeftRight from '@lucide/svelte/icons/arrow-left-right';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Folder from '@lucide/svelte/icons/folder';
  import Grid2x2 from '@lucide/svelte/icons/grid-2x2';
  import Star from '@lucide/svelte/icons/star';
  import UserRoundKey from '@lucide/svelte/icons/user-round-key';
  import type { ConnectionDto, CredentialDto, EntryDto, FolderDto, SavedSplitDto } from '../types';
  import ConnectionIcon from './ConnectionIcon.svelte';
  import { t } from '../i18n/index.svelte';
  import { colorTitle, connectionTooltip, splitCaption, type DropMark, type SidebarRow } from './model';

  interface Props {
    row: SidebarRow;
    /** An entry row's entry. */
    entry?: EntryDto;
    /** A child row's connection, or an entry row's default connection (icon and tooltip). */
    connection?: ConnectionDto;
    /** A port forward of the connection (an entry: of any of its) is starting or active: green arrows. */
    forwardRunning?: boolean;
    /** The connection (an entry: any of its) has a live embedded session: green dot. */
    sessionOpen?: boolean;
    folder?: FolderDto;
    credential?: CredentialDto;
    split?: SavedSplitDto;
    /** The split's existing connections' names. */
    splitNames?: string[];
    selected: boolean;
    drop?: DropMark;
    draggable: boolean;
    onpointer: (event: MouseEvent) => void;
    onactivate: () => void;
    onmenu: (event: MouseEvent) => void;
    ontoggle?: () => void;
    ondragstart?: (event: DragEvent) => void;
    ondragover?: (event: DragEvent) => void;
    ondragleave?: (event: DragEvent) => void;
    ondrop?: (event: DragEvent) => void;
    ondragend?: (event: DragEvent) => void;
  }

  let {
    row,
    entry,
    connection,
    forwardRunning = false,
    sessionOpen = false,
    folder,
    credential,
    split,
    splitNames = [],
    selected,
    drop = null,
    draggable,
    onpointer,
    onactivate,
    onmenu,
    ontoggle,
    ondragstart,
    ondragover,
    ondragleave,
    ondrop,
    ondragend,
  }: Props = $props();

  const tooltip = $derived(
    connection ? connectionTooltip(connection) : credential ? credential.displayUser || undefined : undefined,
  );
</script>

{#snippet indicators()}
  {#if sessionOpen}
    <span class="session-dot" title={t('Open session')} aria-label={t('Open session')} role="img"></span>
  {/if}
  {#if forwardRunning}
    <span class="forward" title={t('Port forward active')} aria-label={t('Port forward active')} role="img">
      <span class="forward-dot"></span>
      <ArrowLeftRight size={11} strokeWidth={2} />
    </span>
  {/if}
{/snippet}

<div
  class="row"
  class:selected
  class:drop-into={drop === 'into'}
  class:drop-before={drop === 'before'}
  class:drop-after={drop === 'after'}
  role="option"
  aria-selected={selected}
  tabindex="-1"
  data-tag={row.tag}
  title={tooltip}
  style:padding-left="{6 + row.depth * 16}px"
  {draggable}
  onmousedown={(e) => {
    // Keep text from being selected by Shift-clicks.
    if (e.shiftKey || e.detail > 1) e.preventDefault();
  }}
  onclick={onpointer}
  ondblclick={onactivate}
  oncontextmenu={onmenu}
  onkeydown={() => {}}
  {ondragstart}
  {ondragover}
  {ondragleave}
  {ondrop}
  {ondragend}
>
  {#if row.kind === 'folder'}
    <button
      type="button"
      class="disclosure"
      class:open={row.expanded}
      class:hidden={!row.hasChildren}
      tabindex="-1"
      aria-label={row.expanded ? t('Collapse') : t('Expand')}
      onclick={(e) => {
        e.stopPropagation();
        ontoggle?.();
      }}
      ondblclick={(e) => e.stopPropagation()}
    >
      <ChevronRight size={12} strokeWidth={2} />
    </button>
    <span
      class="icon folder-icon"
      style:color={folder?.color ? `var(--label-${folder.color})` : 'var(--accent)'}
    >
      <Folder size={15} strokeWidth={1.75} fill={folder?.color ? 'currentColor' : 'none'} />
    </span>
    <span class="name">{folder?.name ?? ''}</span>
  {:else if row.kind === 'entry' && entry}
    {#if row.hasChildren}
      <button
        type="button"
        class="disclosure"
        class:open={row.expanded}
        tabindex="-1"
        aria-label={row.expanded ? t('Collapse') : t('Expand')}
        onclick={(e) => {
          e.stopPropagation();
          ontoggle?.();
        }}
        ondblclick={(e) => e.stopPropagation()}
      >
        <ChevronRight size={12} strokeWidth={2} />
      </button>
    {/if}
    {#if connection}<ConnectionIcon type={connection.type} />{/if}
    <span class="text">
      <span class="title-line">
        <span class="name">{entry.name}</span>
        {#if entry.color}
          <span class="swatch" style:background="var(--label-{entry.color})" title={colorTitle(entry.color)}
          ></span>
        {/if}
      </span>
      {#if row.subtitle}<span class="subtitle">{row.subtitle}</span>{/if}
    </span>
    {@render indicators()}
    {#if entry.isFavorite}
      <span class="star"><Star size={10} fill="currentColor" strokeWidth={0} /></span>
    {/if}
  {:else if row.kind === 'connection' && connection}
    <ConnectionIcon type={connection.type} />
    <span class="text">
      <span class="title-line">
        <span class="name">{row.label}</span>
        {#if row.isDefault}
          <span
            class="default"
            title={t('Default connection')}
            aria-label={t('Default connection')}
            role="img"
          >
            <CircleCheck size={11} strokeWidth={2} />
          </span>
        {/if}
      </span>
    </span>
    {@render indicators()}
  {:else if row.kind === 'credential' && credential}
    <span class="icon credential-icon"><UserRoundKey size={15} strokeWidth={1.75} /></span>
    <span class="name">{credential.name}</span>
    {#if credential.usageCount > 0}<span class="count">{credential.usageCount}</span>{/if}
  {:else if row.kind === 'split' && split}
    <span class="icon split-icon" class:empty={splitNames.length === 0}>
      <Grid2x2 size={15} strokeWidth={1.75} />
    </span>
    <span class="text">
      <span class="name">{split.name}</span>
      <span class="subtitle">{splitCaption(splitNames)}</span>
    </span>
  {/if}
</div>

<style>
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 26px;
    padding: 2px 8px 2px 6px;
    border-radius: 7px;
    cursor: default;
    user-select: none;
    transition: background-color 0.12s ease;
  }
  .row:hover:not(.selected) {
    background: var(--chrome-hover);
  }
  .row.selected {
    background: var(--chrome-selected);
  }
  :global(.sidebar-list:focus-within) .row.selected {
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.12), rgba(255, 255, 255, 0))
      var(--selection-accent);
    box-shadow: inset 0 0.5px 0 rgba(255, 255, 255, 0.22);
    color: #fff;
  }
  :global(.sidebar-list:focus-within) .row.selected .subtitle,
  :global(.sidebar-list:focus-within) .row.selected .count {
    color: rgba(255, 255, 255, 0.8);
  }
  :global(.sidebar-list:focus-within) .row.selected .icon,
  :global(.sidebar-list:focus-within) .row.selected .default,
  :global(.sidebar-list:focus-within) .row.selected :global(.connection-icon) {
    color: #fff;
  }
  .drop-into {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
    background: var(--accent-soft);
  }
  .drop-before::before,
  .drop-after::after {
    content: '';
    position: absolute;
    right: 6px;
    left: 6px;
    height: 2px;
    border-radius: 1px;
    background: var(--accent);
  }
  .drop-before::before {
    top: -1px;
  }
  .drop-after::after {
    bottom: -1px;
  }
  .disclosure {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    margin-left: -4px;
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    opacity: 0.6;
    transition: transform 0.12s ease-out;
  }
  .disclosure.open {
    transform: rotate(90deg);
  }
  .disclosure.hidden {
    visibility: hidden;
  }
  .icon {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 18px;
  }
  .forward {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 3px;
    color: var(--label-green);
  }
  .forward-dot,
  .session-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--label-green);
  }
  .session-dot {
    flex: none;
  }
  .credential-icon {
    color: var(--label-orange);
  }
  .split-icon {
    color: var(--label-green);
  }
  .split-icon.empty {
    color: var(--fg-secondary);
  }
  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }
  .title-line {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row > .name {
    flex: 1;
    min-width: 0;
  }
  .swatch {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 2px;
  }
  .subtitle {
    overflow: hidden;
    color: var(--fg-secondary);
    font-size: var(--font-size-small);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .default {
    display: inline-flex;
    flex: none;
    color: var(--accent);
  }
  .star {
    display: inline-flex;
    flex: none;
    color: var(--label-yellow);
  }
  .count {
    flex: none;
    color: var(--fg-secondary);
    font-size: var(--font-size-small);
    font-variant-numeric: tabular-nums;
  }
</style>
