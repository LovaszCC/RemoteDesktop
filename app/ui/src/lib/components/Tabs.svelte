<script lang="ts">
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import { t } from '../i18n/index.svelte';

  /**
   * A tab strip (WAI-ARIA tabs, automatic activation): ←/→ move and select, Home/End go to the ends,
   * only the selected tab is in the Tab order. A `marked` tab shows the default-connection check.
   */
  interface Item {
    id: string;
    label: string;
    /** Shows the check icon (the default connection). */
    marked?: boolean;
    /** The accessible name when the label alone isn't one (e.g. "+"). */
    title?: string;
  }
  interface Props {
    items: readonly Item[];
    selected: string;
    onselect: (id: string) => void;
    /** The tab list's accessible name. */
    label?: string;
    /** The id of the panel the tabs control. */
    controls?: string;
    /** Each tab button's id is `<idPrefix>-<item id>`, for the panel's `aria-labelledby`. */
    idPrefix?: string;
  }

  let {
    items,
    selected,
    onselect,
    label,
    controls,
    idPrefix = `tab-${Math.random().toString(36).slice(2)}`,
  }: Props = $props();
  let list: HTMLDivElement | undefined = $state();

  function keydown(event: KeyboardEvent, index: number) {
    const last = items.length - 1;
    let next: number;
    switch (event.key) {
      case 'ArrowRight':
        next = index === last ? 0 : index + 1;
        break;
      case 'ArrowLeft':
        next = index === 0 ? last : index - 1;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = last;
        break;
      default:
        return;
    }
    event.preventDefault();
    const item = items[next];
    if (!item) return;
    onselect(item.id);
    list?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus();
  }
</script>

<div class="tabs" role="tablist" aria-label={label} bind:this={list}>
  {#each items as item, index (item.id)}
    <button
      type="button"
      role="tab"
      id={`${idPrefix}-${item.id}`}
      aria-selected={item.id === selected}
      aria-controls={controls}
      aria-label={item.title}
      title={item.title}
      tabindex={item.id === selected ? 0 : -1}
      class:selected={item.id === selected}
      onclick={() => onselect(item.id)}
      onkeydown={(e) => keydown(e, index)}
    >
      <span class="label">{item.label}</span>
      {#if item.marked}
        <span class="mark" title={t('Default connection')} aria-label={t('Default connection')} role="img">
          <CircleCheck size={11} strokeWidth={2} />
        </span>
      {/if}
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 2px;
    border-bottom: 0.5px solid var(--border-control);
  }
  button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 180px;
    min-height: 24px;
    padding: 2px 10px;
    border: none;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--fg-secondary);
    white-space: nowrap;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  button:hover {
    color: inherit;
  }
  .selected {
    border-bottom-color: var(--accent);
    color: inherit;
  }
  button:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--focus-ring);
    border-radius: var(--radius-small);
  }
  .mark {
    display: inline-flex;
    color: var(--accent);
  }
</style>
