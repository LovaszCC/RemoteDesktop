<script lang="ts" generics="T extends string">
  interface Option {
    value: T;
    label: string;
    disabled?: boolean;
  }

  interface Props {
    value: T;
    options: readonly Option[];
    disabled?: boolean;
    label?: string;
    onchange?: (value: T) => void;
  }

  let { value = $bindable(), options, disabled = false, label, onchange }: Props = $props();
</script>

<select class="select" bind:value {disabled} aria-label={label} onchange={() => onchange?.(value)}>
  {#each options as option (option.value)}
    <option value={option.value} disabled={option.disabled}>{option.label}</option>
  {/each}
</select>

<style>
  .select {
    height: var(--control-height);
    padding: 0 6px;
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius);
    background: var(--bg-control);
    box-shadow: 0 0.5px 1px rgba(0, 0, 0, 0.08);
  }
  .select:disabled {
    opacity: 0.5;
  }
</style>
