<script lang="ts">
  import Eye from '@lucide/svelte/icons/eye';
  import EyeOff from '@lucide/svelte/icons/eye-off';
  import type { HTMLInputAttributes } from 'svelte/elements';
  import { t } from '../i18n/index.svelte';
  import TextField from './TextField.svelte';

  interface Props extends Omit<HTMLInputAttributes, 'value' | 'type'> {
    value?: string;
    onsubmit?: () => void;
    invalid?: boolean;
    /** Offer the eye button that shows the text. */
    revealable?: boolean;
    element?: HTMLInputElement;
  }

  let {
    value = $bindable(''),
    revealable = true,
    element = $bindable(),
    class: className = '',
    ...rest
  }: Props = $props();
  let revealed = $state(false);
</script>

<div class="secure-field {className}">
  <TextField bind:value bind:element type={revealed ? 'text' : 'password'} {...rest} />
  {#if revealable}
    <button
      type="button"
      class="reveal"
      tabindex="-1"
      aria-label={revealed ? t('Hide password') : t('Show password')}
      title={revealed ? t('Hide password') : t('Show password')}
      onclick={() => (revealed = !revealed)}
    >
      {#if revealed}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
    </button>
  {/if}
</div>

<style>
  .secure-field {
    position: relative;
    width: 100%;
  }
  .secure-field :global(.text-field) {
    padding-right: 26px;
  }
  .reveal {
    position: absolute;
    top: 50%;
    right: 3px;
    display: inline-flex;
    padding: 3px;
    border: none;
    border-radius: var(--radius-small);
    background: none;
    color: var(--fg-secondary);
    transform: translateY(-50%);
  }
  .reveal:hover {
    color: var(--fg);
  }
</style>
