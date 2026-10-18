<script lang="ts">
  import Info from '@lucide/svelte/icons/info';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import type { Snippet } from 'svelte';

  interface Props {
    kind?: 'info' | 'warning' | 'error';
    message: string;
    /** Buttons on the right. */
    actions?: Snippet;
  }

  let { kind = 'info', message, actions }: Props = $props();
</script>

<div class="banner {kind}" role={kind === 'info' ? 'status' : 'alert'}>
  {#if kind === 'info'}<Info size={14} />{:else}<TriangleAlert size={14} />{/if}
  <span class="message">{message}</span>
  {#if actions}<span class="actions">{@render actions()}</span>{/if}
</div>

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 6px var(--space-3);
    border-bottom: 0.5px solid var(--separator);
    font-size: var(--font-size-callout);
  }
  .message {
    flex: 1;
  }
  .actions {
    display: flex;
    gap: var(--space-2);
  }
  .info {
    background: var(--accent-soft);
  }
  .warning {
    background: var(--warning-soft);
  }
  .warning :global(svg) {
    color: var(--warning);
  }
  .error {
    background: var(--danger-soft);
  }
  .error :global(svg) {
    color: var(--danger);
  }
</style>
