<script lang="ts">
  // While a connect waits for its VPN: which VPN, why (the address that didn't answer the ping), and what
  // MacVPN waits for (a sign-in code or a password, entered in MacVPN's own window). Cancel stops the
  // connect; its error comes back through the connect's own alert.
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import { t } from '../i18n/index.svelte';
  import { vpn as defaultVpn, type VpnStore } from './vpn.svelte';

  let { vpn = defaultVpn }: { vpn?: Pick<VpnStore, 'progress' | 'cancel'> } = $props();

  const progress = $derived(vpn.progress);
  const waiting = $derived(
    progress?.phase === 'waitingForMfa'
      ? t('MacVPN needs your sign-in code — look at its window.')
      : progress?.phase === 'waitingForCredentials'
        ? t('MacVPN needs your password — look at its window.')
        : null,
  );
</script>

{#if progress}
  <Modal title={progress.entryName} width={380}>
    <div class="body" role="status">
      <span class="spin"><LoaderCircle size={16} /></span>
      <div class="text">
        <strong>{t('Connecting VPN "%@"…', progress.profileName)}</strong>
        {#if progress.unreachable}
          <span class="muted">{t('%@ did not answer the ping.', progress.unreachable)}</span>
        {/if}
        {#if waiting}<span>{waiting}</span>{/if}
      </div>
    </div>
    {#snippet footer()}
      <Button onclick={() => void vpn.cancel()}>{t('Cancel')}</Button>
    {/snippet}
  </Modal>
{/if}

<style>
  .body {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .muted {
    color: var(--fg-secondary);
  }
  .spin {
    display: inline-flex;
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
