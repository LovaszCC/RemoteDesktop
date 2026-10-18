<script lang="ts">
  import { toast as defaultToasts, type ToastStore } from '../stores/toast.svelte';

  let { toasts = defaultToasts }: { toasts?: ToastStore } = $props();
</script>

<div class="toast-host" aria-live="polite">
  {#if toasts.current}
    {#key toasts.current.id}
      <button type="button" class="toast" onclick={() => toasts.dismiss()}>{toasts.current.message}</button>
    {/key}
  {/if}
</div>

<style>
  .toast-host {
    position: fixed;
    right: 0;
    bottom: 16px;
    left: 0;
    z-index: 300;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }
  .toast {
    max-width: min(560px, calc(100vw - 32px));
    padding: 8px 14px;
    border: none;
    border-radius: 999px;
    background: var(--glass-surface);
    box-shadow: var(--glass-edge), var(--shadow-popover);
    font-size: var(--font-size-callout);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    pointer-events: auto;
    animation: rise 0.18s ease-out;
  }
  @keyframes rise {
    from {
      transform: translateY(12px);
      opacity: 0;
    }
  }
</style>
