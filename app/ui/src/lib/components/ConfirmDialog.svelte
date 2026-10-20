<script lang="ts">
  import { t } from '../i18n/index.svelte';
  import Button from './Button.svelte';
  import Checkbox from './Checkbox.svelte';
  import Modal from './Modal.svelte';

  interface Props {
    title: string;
    message?: string;
    confirmLabel: string;
    cancelLabel?: string;
    /** Red confirm button. */
    destructive?: boolean;
    /** A checkbox under the message, unchecked at first; its state goes to `onconfirm`. */
    option?: string;
    onconfirm: (checked: boolean) => void;
    oncancel: () => void;
  }

  let {
    title,
    message,
    confirmLabel,
    cancelLabel,
    destructive = false,
    option,
    onconfirm,
    oncancel,
  }: Props = $props();
  let checked = $state(false);
</script>

<Modal {title} onclose={oncancel} width={380}>
  {#if message}<p class="message">{message}</p>{/if}
  {#if option}<div class="option"><Checkbox bind:checked label={option} /></div>{/if}
  {#snippet footer()}
    <Button onclick={oncancel}>{cancelLabel ?? t('Cancel')}</Button>
    <Button variant={destructive ? 'destructive' : 'primary'} onclick={() => onconfirm(checked)} autofocus>
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
  .option {
    margin-top: var(--space-3);
  }
</style>
