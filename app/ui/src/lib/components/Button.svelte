<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { HTMLButtonAttributes } from 'svelte/elements';

  interface Props extends HTMLButtonAttributes {
    /** `primary` is the default action (accent), `destructive` is red, `link` looks like a link. */
    variant?: 'default' | 'primary' | 'destructive' | 'link';
    children: Snippet;
  }

  let { variant = 'default', type = 'button', children, class: className = '', ...rest }: Props = $props();
</script>

<button {type} class="button {variant} {className}" {...rest}>
  {@render children()}
</button>

<style>
  .button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    min-height: var(--control-height);
    padding: 2px 12px;
    border: 0.5px solid var(--border-control);
    border-radius: 7px;
    background: var(--bg-control);
    box-shadow: 0 0.5px 1px rgba(15, 23, 42, 0.08);
    white-space: nowrap;
    cursor: default;
    transition:
      background-color 0.15s ease,
      filter 0.15s ease,
      transform 0.1s ease;
  }
  .button:hover:not(:disabled) {
    background: var(--bg-control-hover);
  }
  .button:active:not(:disabled) {
    filter: brightness(0.95);
    transform: translateY(0.5px);
  }
  .button:disabled {
    opacity: 0.45;
  }
  .primary {
    border-color: transparent;
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.14), rgba(255, 255, 255, 0)) var(--accent);
    box-shadow:
      inset 0 0.5px 0 rgba(255, 255, 255, 0.25),
      0 1px 2px color-mix(in srgb, var(--accent) 35%, transparent);
    color: var(--accent-fg);
  }
  .primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .destructive {
    border-color: transparent;
    background: var(--danger);
    color: #fff;
  }
  .destructive:hover:not(:disabled) {
    background: var(--danger);
    filter: brightness(1.08);
  }
  .link {
    min-height: 0;
    padding: 0;
    border: none;
    background: none;
    box-shadow: none;
    color: var(--accent);
  }
  .link:hover:not(:disabled) {
    background: none;
    text-decoration: underline;
  }
</style>
