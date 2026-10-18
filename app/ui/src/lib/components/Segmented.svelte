<script lang="ts" generics="T extends string">
  /** A segmented picker (SwiftUI `.pickerStyle(.segmented)`). */
  interface Props {
    value: T;
    options: readonly { value: T; label: string }[];
    label?: string;
    disabled?: boolean;
    onchange?: (value: T) => void;
  }

  let { value = $bindable(), options, label, disabled = false, onchange }: Props = $props();

  function choose(next: T) {
    value = next;
    onchange?.(next);
  }
</script>

<div class="segmented" role="radiogroup" aria-label={label}>
  {#each options as option (option.value)}
    <button
      type="button"
      role="radio"
      aria-checked={option.value === value}
      class:selected={option.value === value}
      {disabled}
      onclick={() => choose(option.value)}
    >
      {option.label}
    </button>
  {/each}
</div>

<style>
  .segmented {
    display: flex;
    padding: 2px;
    border-radius: var(--radius);
    background: var(--bg-selected);
  }
  button {
    flex: 1;
    min-height: 20px;
    padding: 1px 10px;
    border: none;
    border-radius: var(--radius-small);
    background: none;
    white-space: nowrap;
  }
  .selected {
    background: var(--bg-control);
    box-shadow: 0 0.5px 2px rgba(0, 0, 0, 0.2);
  }
  button:disabled {
    opacity: 0.5;
  }
</style>
