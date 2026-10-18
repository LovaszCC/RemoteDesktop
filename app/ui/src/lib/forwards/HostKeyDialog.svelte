<script lang="ts">
  // An unknown host key of a background connection (Swift "Unknown host key" alert): the host, the key
  // type and the SHA256 fingerprint in monospace; Accept appends it to known_hosts, Reject (or Esc) stops
  // the tunnel. A changed key (`changed` set) shows a warning with the stored entry instead; "Replace key"
  // swaps the stored key for the new one, Cancel (or Esc) refuses. Embedded terminals reuse it for the
  // changed key the system `ssh` refused.
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import { t } from '../i18n/index.svelte';
  import { hostKeys as defaultStore, type HostKeyPrompts } from './hostKeys.svelte';

  let { store = defaultStore, acceptLabel }: { store?: HostKeyPrompts; acceptLabel?: string } = $props();

  const prompt = $derived(store.current);
  const hostName = $derived(
    prompt ? (prompt.port === 22 ? prompt.host : `[${prompt.host}]:${prompt.port}`) : '',
  );
</script>

{#if prompt}
  {#key prompt.requestId}
    {#if prompt.changed}
      <Modal title={t('Host key changed')} onclose={() => void store.answer(false)} width={560}>
        <p class="message warning" role="alert">
          <span class="icon"><TriangleAlert size={16} /></span>
          <span>
            {t(
              'The host key of %@ has changed. This can mean someone is intercepting the connection. Replace the stored key only if you expect the change.',
              hostName,
            )}
          </span>
        </p>
        <dl>
          <dt>{t('Host')}</dt>
          <dd class="selectable">{hostName}</dd>
          <dt>{t('Key type')}</dt>
          <dd class="selectable">{prompt.keyType}</dd>
          <dt>{t('New fingerprint')}</dt>
          <dd class="mono selectable" data-testid="host-key-fingerprint">{prompt.fingerprint}</dd>
          <dt>{t('Stored key')}</dt>
          <dd class="mono selectable" data-testid="host-key-stored">
            {prompt.changed.file}:{prompt.changed.line}
          </dd>
        </dl>
        {#snippet footer()}
          <Button onclick={() => void store.answer(false)}>{t('Cancel')}</Button>
          <Button variant="destructive" onclick={() => void store.answer(true)}>{t('Replace key')}</Button>
        {/snippet}
      </Modal>
    {:else}
      <Modal title={t('Unknown host key')} onclose={() => void store.answer(false)} width={520}>
        <p class="message">
          {t(
            "%@ isn't in known_hosts yet. Accept only if this fingerprint matches the server's key.",
            hostName,
          )}
        </p>
        <dl>
          <dt>{t('Host')}</dt>
          <dd class="selectable">{hostName}</dd>
          <dt>{t('Key type')}</dt>
          <dd class="selectable">{prompt.keyType}</dd>
          <dt>{t('Fingerprint')}</dt>
          <dd class="mono selectable" data-testid="host-key-fingerprint">{prompt.fingerprint}</dd>
        </dl>
        {#snippet footer()}
          <Button onclick={() => void store.answer(false)}>{t('Reject')}</Button>
          <Button variant="primary" onclick={() => void store.answer(true)}
            >{acceptLabel ?? t('Accept and Start')}</Button
          >
        {/snippet}
      </Modal>
    {/if}
  {/key}
{/if}

<style>
  .message {
    margin: 0 0 var(--space-3);
    overflow-wrap: anywhere;
  }
  .warning {
    display: flex;
    gap: var(--space-2);
    align-items: flex-start;
  }
  .icon {
    display: inline-flex;
    flex: none;
    color: var(--label-red);
  }
  dl {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 6px var(--space-3);
    margin: 0;
  }
  dt {
    color: var(--fg-secondary);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: var(--font-size-callout);
  }
</style>
