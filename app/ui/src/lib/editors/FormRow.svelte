<script lang="ts">
  import type { Snippet } from 'svelte';

  /** One labelled form row; `error` shows in red under the control, `hint` in grey. */
  interface Props {
    label?: string;
    /** The `id` of the control the label names. */
    for?: string;
    error?: string | null;
    hint?: string | null;
    children: Snippet;
  }

  let { label, for: control, error = null, hint = null, children }: Props = $props();
</script>

<div class="form-row">
  {#if label}
    <label class="label" for={control}>{label}</label>
  {/if}
  <div class="control" class:wide={!label}>
    {@render children()}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if hint}<p class="hint">{hint}</p>{/if}
  </div>
</div>

<style>
  .form-row {
    display: grid;
    grid-template-columns: 130px minmax(0, 1fr);
    align-items: baseline;
    gap: var(--space-3);
    padding: 7px 0;
  }
  .label {
    padding-top: 3px;
  }
  .control {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 4px;
    min-width: 0;
  }
  .wide {
    grid-column: 1 / -1;
  }
  p {
    margin: 0;
    font-size: var(--font-size-small);
  }
  .error {
    color: var(--danger);
  }
  .hint {
    color: var(--fg-secondary);
  }
</style>
