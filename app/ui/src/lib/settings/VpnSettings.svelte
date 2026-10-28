<script lang="ts">
  // Settings → VPN (macOS): pairing with MacVPN, and the VPNs RemoteDesktop holds up. "Release" lets go of
  // what external launches hold; open tabs keep theirs (the row says how many).
  import Button from '../components/Button.svelte';
  import FormRow from '../editors/FormRow.svelte';
  import FormSection from '../editors/FormSection.svelte';
  import { errorMessage } from '../errors';
  import { t, tn } from '../i18n/index.svelte';
  import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
  import { dialogs as defaultDialogs, type DialogStore } from '../stores/dialogs.svelte';
  import { vpn as defaultVpn, type VpnStore } from '../vpn/vpn.svelte';

  interface Props {
    vpn?: VpnStore;
    dialogs?: Pick<DialogStore, 'confirm'>;
    alerts?: Pick<AlertStore, 'show'>;
  }

  let { vpn = defaultVpn, dialogs = defaultDialogs, alerts = defaultAlerts }: Props = $props();
  let pairing = $state(false);

  const status = $derived(vpn.status);

  async function perform(work: () => Promise<unknown>) {
    try {
      await work();
    } catch (error) {
      alerts.show(errorMessage(error));
    }
  }

  async function pair() {
    pairing = true;
    await perform(() => vpn.pair());
    pairing = false;
  }

  async function unpair() {
    const ok = await dialogs.confirm({
      title: t('Unpair from MacVPN?'),
      message: t('RemoteDesktop forgets its MacVPN token. Revoke it in MacVPN → Paired apps too.'),
      confirmLabel: t('Unpair'),
      destructive: true,
    });
    if (ok) await perform(() => vpn.unpair());
  }
</script>

<FormSection title={t('MacVPN')}>
  {#if status?.paired}
    <FormRow>
      <div class="state">
        <span class="paired">● {t('Paired with MacVPN')}</span>
        <span class="hint">{t('Choose the VPNs RemoteDesktop may use in MacVPN → Paired apps.')}</span>
        <div class="buttons">
          <Button disabled={pairing} onclick={() => void pair()}>{t('Pair again…')}</Button>
          <Button onclick={() => void unpair()}>{t('Unpair')}</Button>
        </div>
      </div>
    </FormRow>
  {:else}
    <FormRow>
      <div class="state">
        <span>{t('Not paired.')}</span>
        <span class="hint">
          {t(
            'In MacVPN: Manage Connections… → Paired apps → turn on "Allow other apps to control VPNs". Then pair, and tick the VPNs RemoteDesktop may use.',
          )}
        </span>
        <div class="buttons">
          <Button variant="primary" disabled={pairing} onclick={() => void pair()}>
            {t('Pair with MacVPN…')}
          </Button>
        </div>
      </div>
    </FormRow>
  {/if}
  {#if pairing}
    <FormRow><span class="hint">{t('Waiting for MacVPN… Allow the pairing in its window.')}</span></FormRow>
  {/if}
</FormSection>

<FormSection title={t('Held by RemoteDesktop')}>
  {#if !status || status.held.length === 0}
    <FormRow><span class="hint">{t('No VPN is held.')}</span></FormRow>
  {:else}
    {#each status.held as held (held.profileId)}
      <div class="lease">
        <div class="grow">
          <strong>{held.profileName}</strong>
          {#if held.externalEntries.length > 0}
            <span class="hint">{held.externalEntries.map((e) => e.entryName).join(', ')}</span>
          {/if}
          {#if held.openHolders > 0}
            <span class="hint">{tn('Still used by %lld open tabs', held.openHolders)}</span>
          {/if}
        </div>
        <Button
          disabled={held.externalEntries.length === 0}
          onclick={() => void perform(() => vpn.releaseProfile(held.profileId))}
        >
          {t('Release')}
        </Button>
      </div>
    {/each}
  {/if}
</FormSection>

<style>
  .state {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .paired {
    color: var(--label-green);
  }
  .hint {
    color: var(--fg-secondary);
    font-size: var(--font-size-small);
  }
  .buttons {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-1);
  }
  .lease {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2);
    border: 0.5px solid var(--separator);
    border-radius: 6px;
    margin-bottom: var(--space-1);
  }
  .grow {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
  }
</style>
