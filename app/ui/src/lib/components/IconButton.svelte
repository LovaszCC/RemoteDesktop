<script lang="ts">
  import type { Component } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  interface Props extends HTMLButtonAttributes {
    /** A `@lucide/svelte` icon. */
    icon: Component<{ size?: number | string; strokeWidth?: number | string }>;
    /** Accessible name and tooltip. */
    label: string;
    size?: number;
    /** Shown pressed (a toggled toolbar button). */
    active?: boolean;
  }

  let { icon: Icon, label, size = 16, active = false, type = 'button', ...rest }: Props = $props();
</script>

<button {type} class="icon-button" class:active aria-label={label} title={label} {...rest}>
  <Icon {size} strokeWidth={1.75} />
</button>

<style>
  .icon-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 28px;
    height: 28px;
    padding: 0 6px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--fg-secondary);
    cursor: default;
    transition:
      background-color 0.15s ease,
      color 0.15s ease,
      transform 0.1s ease;
  }
  .icon-button:hover:not(:disabled),
  .active {
    background: var(--chrome-hover);
    color: var(--fg);
  }
  .icon-button:active:not(:disabled) {
    transform: scale(0.94);
  }
  .icon-button:disabled {
    opacity: 0.4;
  }
</style>
