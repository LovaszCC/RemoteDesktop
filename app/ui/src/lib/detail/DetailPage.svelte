<script lang="ts">
  import type { Snippet } from 'svelte';

  /** The scrolling page of a detail view: a header (big icon, title, subtitle, accessory) and sections. */
  interface Props {
    title: string;
    subtitle?: string;
    icon: Snippet;
    accessory?: Snippet;
    children: Snippet;
  }

  let { title, subtitle, icon, accessory, children }: Props = $props();
</script>

<div class="detail-page">
  <header>
    <span class="icon">{@render icon()}</span>
    <div class="titles">
      <h1 class="selectable">{title}</h1>
      {#if subtitle}<p>{subtitle}</p>{/if}
    </div>
    {#if accessory}<span class="accessory">{@render accessory()}</span>{/if}
  </header>
  {@render children()}
</div>

<style>
  .detail-page {
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: var(--space-5);
  }
  header {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .icon {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 44px;
  }
  .titles {
    flex: 1;
    min-width: 0;
  }
  h1 {
    margin: 0;
    overflow: hidden;
    font-size: var(--font-size-large-title);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  p {
    margin: 2px 0 0;
    color: var(--fg-secondary);
  }
  /* Shared by the detail views. */
  .detail-page :global(.actions) {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .detail-page :global(.actions .button) {
    min-height: 28px;
    padding: 3px 14px;
  }
  .detail-page :global(.grid) {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 10px 16px;
    align-items: baseline;
  }
  .detail-page :global(.grid > .label) {
    color: var(--fg-secondary);
    text-align: right;
  }
  .detail-page :global(.grid > .value) {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .detail-page :global(section) {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .detail-page :global(section > h2),
  .detail-page :global(.section-head h2) {
    margin: 0;
    font-size: var(--font-size);
    font-weight: 600;
  }
  .detail-page :global(.section-head) {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .detail-page :global(.section-head h2) {
    flex: 1;
  }
  .detail-page :global(.notes) {
    margin: 0;
    white-space: pre-wrap;
  }
  .detail-page :global(.muted) {
    color: var(--fg-secondary);
  }
  .detail-page :global(.caption) {
    color: var(--fg-secondary);
    font-size: var(--font-size-small);
  }
</style>
