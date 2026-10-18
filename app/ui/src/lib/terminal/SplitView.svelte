<script lang="ts">
  // Swift `SplitSessionView`: the toolbar ("Add terminal", n/4, "Broadcast to all panes") above the pane
  // grid (`splitColumns`: 2 side by side, 1 + 2, 2×2).
  import Plus from '@lucide/svelte/icons/plus';
  import RadioTower from '@lucide/svelte/icons/radio-tower';
  import Button from '../components/Button.svelte';
  import { showContextMenu } from '../components/menu.svelte';
  import { t } from '../i18n/index.svelte';
  import { library as defaultLibrary, type LibraryStore } from '../stores/library.svelte';
  import { settings as defaultSettings, type SettingsStore } from '../stores/settings.svelte';
  import type { SessionsStore } from './sessions.svelte';
  import { LOCAL_TERMINAL } from './local';
  import { MAX_PANES, addTerminalItems, isAllBroadcasting, splitColumns } from './split';
  import SplitPaneView from './SplitPaneView.svelte';
  import type { SplitsStore } from './splits.svelte';

  interface Props {
    splits: SplitsStore;
    sessions: SessionsStore;
    id: string;
    active: boolean;
    library?: Pick<LibraryStore, 'library' | 'connection'>;
    settings?: SettingsStore;
  }

  let {
    splits,
    sessions,
    id,
    active,
    library = defaultLibrary,
    settings = defaultSettings,
  }: Props = $props();

  const split = $derived(splits.get(id));
  const full = $derived(split ? split.panes.length >= MAX_PANES : true);
  const allBroadcasting = $derived(split ? isAllBroadcasting(split) : false);
  const columns = $derived(split ? splitColumns(split.panes.length) : []);
  const broadcastHint = $derived(settings.shortcut('broadcastAll')?.display ?? '');

  function openAddMenu(event: MouseEvent) {
    if (!split) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    showContextMenu(
      { clientX: rect.left, clientY: rect.bottom + 2 },
      addTerminalItems(
        split,
        library.library.connections,
        (c) => void splits.addPane(id, c),
        () => void splits.addPane(id, LOCAL_TERMINAL),
      ),
    );
  }
</script>

{#if split}
  <div class="split" data-split={id}>
    <div class="toolbar">
      <Button
        disabled={full}
        aria-haspopup="menu"
        title={full
          ? t('A split view holds at most %lld terminals.', MAX_PANES)
          : t('Add a terminal to the split view')}
        onclick={openAddMenu}
      >
        <Plus size={13} />{t('Add terminal')}
      </Button>
      <span class="count">{split.panes.length}/{MAX_PANES}</span>
      <span class="spacer"></span>
      <button
        type="button"
        class="broadcast-all"
        class:on={allBroadcasting}
        aria-pressed={allBroadcasting}
        title={t('Type into every pane at once (%@)', broadcastHint)}
        onclick={() => splits.toggleBroadcastAll(id)}
      >
        <RadioTower size={13} />{t('Broadcast to all panes')}
      </button>
    </div>
    <div class="grid">
      {#each columns as column, c (c)}
        <div class="column">
          {#each column as index (split.panes[index]?.id ?? index)}
            {@const pane = split.panes[index]}
            {#if pane}
              <SplitPaneView
                {splits}
                {sessions}
                {pane}
                focused={split.focused === pane.id}
                broadcasting={split.broadcasting.includes(pane.id)}
                active={active && split.focused === pane.id}
                {library}
                {settings}
              />
            {/if}
          {/each}
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .split {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--space-2);
    padding: 4px 10px;
    border-bottom: 0.5px solid var(--separator);
    background: var(--bg-toolbar);
    font-size: var(--font-size-callout);
  }
  .toolbar :global(.button) {
    min-height: 20px;
    padding: 1px 8px;
    font-size: var(--font-size-callout);
  }
  .count {
    color: var(--fg-secondary);
    font-variant-numeric: tabular-nums;
  }
  .spacer {
    flex: 1;
  }
  .broadcast-all {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    min-height: 20px;
    padding: 1px 8px;
    border: 0.5px solid var(--separator);
    border-radius: var(--radius-small);
    background: transparent;
    color: var(--fg);
    font: inherit;
  }
  .broadcast-all.on {
    border-color: var(--label-orange);
    background: color-mix(in srgb, var(--label-orange) 22%, transparent);
    color: var(--label-orange);
  }
  .grid {
    display: flex;
    flex: 1;
    gap: 2px;
    min-height: 0;
    background: var(--separator);
  }
  .column {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
</style>
