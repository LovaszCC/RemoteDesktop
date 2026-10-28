<script lang="ts">
  // Swift `SessionTabBar`: "Details" then one tab per session. Double-click or "Rename…" edits the name
  // inline (Enter or blur saves, Esc cancels); the context menu sets the tab colour. A root shell shows a
  // warning icon and tints the tab orange (unless the tab has its own colour).
  import Columns2 from '@lucide/svelte/icons/columns-2';
  import Grid2x2 from '@lucide/svelte/icons/grid-2x2';
  import Info from '@lucide/svelte/icons/info';
  import SquareSplitHorizontal from '@lucide/svelte/icons/square-split-horizontal';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import X from '@lucide/svelte/icons/x';
  import { tick } from 'svelte';
  import { showContextMenu, type MenuItem } from '../components/menu.svelte';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import { colorTitle } from '../sidebar/model';
  import { settings as defaultSettings, type SettingsStore } from '../stores/settings.svelte';
  import { tabs as defaultTabs, type TabSession, type TabsStore } from '../tabs.svelte';
  import SplitTabsDialog from '../terminal/SplitTabsDialog.svelte';
  import { splits as defaultSplits, type SplitsStore } from '../terminal/splits.svelte';
  import { LABEL_COLORS, type LabelColor } from '../types';

  interface Props {
    tabs?: TabsStore;
    settings?: SettingsStore;
    splits?: Pick<
      SplitsStore,
      | 'canSplitActive'
      | 'splitActive'
      | 'save'
      | 'canUpdateSaved'
      | 'updateSaved'
      | 'mergeCandidates'
      | 'mergeTabs'
    >;
  }

  let { tabs = defaultTabs, settings = defaultSettings, splits = defaultSplits }: Props = $props();

  let renaming = $state<string | null>(null);
  let draft = $state('');
  let field: HTMLInputElement | undefined = $state();
  /** "Split tabs…" is open. */
  let picking = $state(false);

  const closeHint = $derived(settings.shortcut('closeTab')?.display ?? null);
  const splitHint = $derived(settings.shortcut('splitTerminal')?.display ?? '');
  const mergeCandidates = $derived(
    splits.mergeCandidates.flatMap((id) => {
      const session = tabs.session(id);
      return session ? [session] : [];
    }),
  );

  function background(active: boolean, color: LabelColor | null): string {
    if (color) return `color-mix(in srgb, var(--label-${color}) ${active ? 32 : 14}%, transparent)`;
    return active ? 'color-mix(in srgb, var(--accent) 18%, transparent)' : 'transparent';
  }

  async function startRenaming(session: TabSession) {
    draft = tabs.displayTitle(session);
    renaming = session.id;
    await tick();
    field?.focus();
    field?.select();
  }

  function finishRenaming(save: boolean) {
    const id = renaming;
    if (id === null) return;
    renaming = null;
    if (save) tabs.rename(id, draft);
  }

  function renameKey(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      finishRenaming(true);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      finishRenaming(false);
    }
  }

  function menu(session: TabSession): MenuItem[] {
    const current = tabs.displayColor(session);
    const splitItems: MenuItem[] = session.split
      ? [
          { label: t('Save split…'), action: () => void splits.save(session.id) },
          ...(splits.canUpdateSaved(session.id)
            ? [{ label: t('Update saved split'), action: () => void splits.updateSaved(session.id) }]
            : []),
        ]
      : [];
    return [
      { label: t('Rename…'), action: () => void startRenaming(session) },
      ...splitItems,
      {
        label: t('Color'),
        submenu: [
          { label: t('Entry color'), action: () => tabs.resetColor(session.id) },
          { kind: 'separator' },
          { label: t('None'), checked: current === null, action: () => tabs.setColor(null, session.id) },
          ...LABEL_COLORS.map((color): MenuItem => ({
            label: colorTitle(color),
            checked: current === color,
            action: () => tabs.setColor(color, session.id),
          })),
        ],
      },
    ];
  }
</script>

<div class="tab-bar" role="tablist">
  <button
    type="button"
    role="tab"
    class="tab"
    aria-selected={tabs.active === null}
    style:background={background(tabs.active === null, null)}
    onclick={() => tabs.select(null)}
  >
    <Info size={14} />
    <span class="title">{t('Details')}</span>
  </button>
  {#each tabs.sessions as session (session.id)}
    {@const title = tabs.displayTitle(session)}
    {@const color = tabs.displayColor(session)}
    {@const active = tabs.active === session.id}
    <div
      role="tab"
      tabindex="0"
      class="tab"
      aria-selected={active}
      title={title === session.title ? undefined : session.title}
      class:root={session.root}
      class:broadcasting={session.broadcasting}
      style:background={background(active, color ?? (session.root || session.broadcasting ? 'orange' : null))}
      onclick={() => tabs.select(session.id)}
      ondblclick={() => void startRenaming(session)}
      onkeydown={(e) => {
        if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) {
          e.preventDefault();
          tabs.select(session.id);
        }
      }}
      oncontextmenu={(e) => showContextMenu(e, menu(session))}
    >
      {#if session.root}
        <span class="root-icon" title={t('Root: commands run with full privileges')}>
          <TriangleAlert size={14} />
        </span>
      {:else if session.split}
        <span class="split-icon" class:on={session.broadcasting}><Grid2x2 size={14} /></span>
      {:else}
        <ConnectionIcon type={session.type} size={14} />
      {/if}
      {#if renaming === session.id}
        <input
          bind:this={field}
          bind:value={draft}
          class="rename"
          placeholder={t('Tab name')}
          aria-label={t('Tab name')}
          onkeydown={renameKey}
          onblur={() => finishRenaming(true)}
          onclick={(e) => e.stopPropagation()}
          ondblclick={(e) => e.stopPropagation()}
        />
      {:else}
        <span class="title">{title}</span>
      {/if}
      <button
        type="button"
        class="close"
        aria-label={t('Close session')}
        title={closeHint ? t('Close session (%@)', closeHint) : t('Close session')}
        onclick={(e) => {
          e.stopPropagation();
          tabs.closeSession(session.id);
        }}
      >
        <X size={11} strokeWidth={2.5} />
      </button>
      {#if color}<span class="underline" style:background="var(--label-{color})"></span>{/if}
    </div>
  {/each}
  <span class="spacer"></span>
  {#if splits.canSplitActive}
    <button
      type="button"
      class="split-button"
      aria-label={t('Split terminal')}
      title={t('Open the same server in a new pane next to it (%@)', splitHint)}
      onclick={() => void splits.splitActive()}
    >
      <Columns2 size={14} />
    </button>
  {/if}
  {#if mergeCandidates.length >= 2}
    <button
      type="button"
      class="split-button"
      aria-label={t('Split tabs…')}
      title={t('Put open tabs side by side')}
      onclick={() => (picking = true)}
    >
      <SquareSplitHorizontal size={14} />
    </button>
  {/if}
</div>

{#if picking}
  <SplitTabsDialog
    candidates={mergeCandidates}
    titleOf={(session) => tabs.displayTitle(session)}
    initial={tabs.active === null ? [] : [tabs.active]}
    onconfirm={(ids) => {
      picking = false;
      splits.mergeTabs(ids);
    }}
    oncancel={() => (picking = false)}
  />
{/if}

<style>
  .tab-bar {
    display: flex;
    flex: none;
    gap: 4px;
    align-items: center;
    height: var(--tabbar-height);
    padding: 0 8px;
    overflow-x: auto;
    scrollbar-width: none;
    border-bottom: 0.5px solid var(--separator);
    background: var(--bg-toolbar);
  }
  .tab[aria-selected='true'] {
    box-shadow:
      0 0 0 0.5px var(--separator),
      0 1px 2px rgba(15, 23, 42, 0.08);
  }
  .tab {
    position: relative;
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px;
    border: none;
    border-radius: 7px;
    transition: background-color 0.15s ease;
    color: var(--fg);
    font: inherit;
    cursor: default;
  }
  .tab:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .title {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rename {
    width: 160px;
    padding: 0;
    border: none;
    outline: none;
    background: transparent;
    color: inherit;
    font: inherit;
  }
  .root-icon {
    display: inline-flex;
    color: var(--label-orange);
  }
  .split-icon {
    display: inline-flex;
    color: var(--label-green);
  }
  .split-icon.on {
    color: var(--label-orange);
  }
  .tab.broadcasting {
    box-shadow: inset 0 0 0 1px var(--label-orange);
  }
  .spacer {
    flex: 1;
  }
  .split-button {
    display: inline-flex;
    flex: none;
    padding: 3px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--fg-secondary);
  }
  .split-button:hover {
    background: var(--bg-hover);
    color: var(--fg);
  }
  .tab.root {
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--label-orange) 60%, transparent);
  }
  .close {
    display: inline-flex;
    padding: 2px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--fg-secondary);
  }
  .close:hover {
    background: var(--bg-hover);
    color: var(--fg);
  }
  .underline {
    position: absolute;
    right: 6px;
    bottom: 0;
    left: 6px;
    height: 2px;
    border-radius: 1px;
  }
</style>
