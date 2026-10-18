<script lang="ts">
  import type { HTMLInputAttributes } from 'svelte/elements';

  interface Props extends Omit<HTMLInputAttributes, 'value'> {
    value?: string;
    /** Called on Enter. */
    onsubmit?: () => void;
    /** Red border. */
    invalid?: boolean;
    element?: HTMLInputElement;
  }

  let {
    value = $bindable(''),
    onsubmit,
    invalid = false,
    element = $bindable(),
    type = 'text',
    class: className = '',
    ...rest
  }: Props = $props();

  function keydown(event: KeyboardEvent) {
    if (event.key === 'Enter' && onsubmit) {
      event.preventDefault();
      onsubmit();
    }
  }
</script>

<input
  bind:this={element}
  bind:value
  {type}
  class="text-field {className}"
  class:invalid
  aria-invalid={invalid || undefined}
  autocomplete="off"
  autocapitalize="off"
  spellcheck="false"
  onkeydown={keydown}
  {...rest}
/>

<style>
  .text-field {
    width: 100%;
    height: var(--control-height);
    padding: 2px 7px;
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-small);
    background: var(--bg-control);
    outline: none;
  }
  .text-field:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus-ring);
  }
  .text-field::placeholder {
    color: var(--fg-tertiary);
  }
  .invalid {
    border-color: var(--danger);
  }
  .text-field:disabled {
    opacity: 0.5;
  }
</style>
