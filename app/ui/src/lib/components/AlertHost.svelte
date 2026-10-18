<script lang="ts">
  import { t } from '../i18n/index.svelte';
  import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
  import Button from './Button.svelte';
  import Modal from './Modal.svelte';

  let { alerts = defaultAlerts }: { alerts?: AlertStore } = $props();
</script>

{#if alerts.current}
  <Modal title="RemoteDesktop" onclose={() => alerts.dismiss()} width={360}>
    <p class="message selectable">{alerts.current}</p>
    {#snippet footer()}
      <Button variant="primary" onclick={() => alerts.dismiss()} autofocus>{t('OK')}</Button>
    {/snippet}
  </Modal>
{/if}

<style>
  .message {
    margin: 0;
    white-space: pre-line;
  }
</style>
