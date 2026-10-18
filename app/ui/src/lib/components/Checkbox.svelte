<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    checked?: boolean;
    disabled?: boolean;
    label?: string;
    children?: Snippet;
    onchange?: (checked: boolean) => void;
  }

  let { checked = $bindable(false), disabled = false, label, children, onchange }: Props = $props();
</script>

<label class="checkbox" class:disabled>
  <input type="checkbox" bind:checked {disabled} onchange={() => onchange?.(checked)} />
  <span>
    {#if children}{@render children()}{:else}{label}{/if}
  </span>
</label>

<style>
  .checkbox {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  input {
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--accent);
  }
  .disabled {
    opacity: 0.5;
  }
</style>
