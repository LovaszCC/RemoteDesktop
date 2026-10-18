<script lang="ts">
  import type { Snippet } from 'svelte';

  /** A grouped form section (SwiftUI `Section` in `.formStyle(.grouped)`): an optional title over a box. */
  let { title, children }: { title?: string; children: Snippet } = $props();
</script>

<section class="form-section">
  {#if title}<h3>{title}</h3>{/if}
  <div class="box">{@render children()}</div>
</section>

<style>
  .form-section {
    margin-bottom: var(--space-3);
  }
  h3 {
    margin: 0 0 4px 4px;
    color: var(--fg-secondary);
    font-size: var(--font-size-callout);
    font-weight: 600;
  }
  .box {
    display: flex;
    flex-direction: column;
    padding: 2px 10px;
    border: 0.5px solid var(--separator);
    border-radius: var(--radius);
    background: var(--bg-hover);
  }
  .box > :global(*:not(:last-child)) {
    border-bottom: 0.5px solid var(--separator);
  }
</style>
