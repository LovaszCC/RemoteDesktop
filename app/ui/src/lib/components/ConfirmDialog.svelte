<script lang="ts">
  import { t } from '../i18n/index.svelte';
  import Button from './Button.svelte';
  import Modal from './Modal.svelte';

  interface Props {
    title: string;
    message?: string;
    confirmLabel: string;
    cancelLabel?: string;
    /** Red confirm button. */
    destructive?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  }

  let {
    title,
    message,
    confirmLabel,
    cancelLabel,
    destructive = false,
    onconfirm,
    oncancel,
  }: Props = $props();
</script>

<Modal {title} onclose={oncancel} width={380}>
  {#if message}<p class="message">{message}</p>{/if}
  {#snippet footer()}
    <Button onclick={oncancel}>{cancelLabel ?? t('Cancel')}</Button>
    <Button variant={destructive ? 'destructive' : 'primary'} onclick={onconfirm} autofocus>
      {confirmLabel}
    </Button>
  {/snippet}
</Modal>

<style>
  .message {
    margin: 0;
    color: var(--fg-secondary);
    white-space: pre-line;
  }
</style>
