<script lang="ts">
  // The serial port picker: the plugged-in USB-serial adapters (Refresh lists them again; a port another
  // tab holds says which), plus the settings for an ad-hoc console.
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import { t } from '../i18n/index.svelte';
  import type { SerialPortDto } from '../types';
  import { DEFAULT_SERIAL_SETTINGS, serialForm, settingsFromForm, type SerialForm } from './serial';
  import type { ConnectAnswer } from './serialConsoles.svelte';
  import SerialSettingsFields from './SerialSettingsFields.svelte';

  interface Props {
    /** Ad hoc: show the settings too. */
    adHoc: boolean;
    name: string;
    ports: readonly SerialPortDto[];
    onrefresh: () => void;
    onanswer: (answer: ConnectAnswer | null) => void;
  }

  let { adHoc, name, ports, onrefresh, onanswer }: Props = $props();

  let chosen = $state<string | null>(null);
  let form = $state<SerialForm>(serialForm(DEFAULT_SERIAL_SETTINGS));
  // The choice, else (none yet, or its device is gone after Refresh) the first free port.
  const selected = $derived(
    ports.some((p) => p.path === chosen)
      ? chosen
      : ((ports.find((p) => p.inUseBy === null) ?? ports[0])?.path ?? null),
  );
  const settings = $derived(settingsFromForm(form));
  const baudError = $derived(settings === null ? t('Enter a positive whole number.') : null);
  const ready = $derived(selected !== null && (!adHoc || settings !== null));

  function connect() {
    const port = ports.find((p) => p.path === selected);
    if (!port || !ready) return;
    onanswer({ port, settings: adHoc ? settings : null });
  }
</script>

<Modal
  title={adHoc ? t('Serial Console') : t('Connect “%@”', name)}
  onclose={() => onanswer(null)}
  width={440}
>
  <div class="ports-head">
    <span>{t('USB serial port')}</span>
    <Button title={t('Refresh')} onclick={onrefresh}><RefreshCw size={13} /></Button>
  </div>
  {#if ports.length === 0}
    <p class="empty" role="status">{t('No USB serial device found. Plug in the adapter, then Refresh.')}</p>
  {:else}
    <ul class="ports" role="radiogroup" aria-label={t('USB serial port')}>
      {#each ports as port (port.path)}
        <li>
          <label class="port">
            <input
              type="radio"
              name="serial-port"
              value={port.path}
              checked={port.path === selected}
              onchange={() => (chosen = port.path)}
            />
            <span class="path">{port.path}</span>
            <span class="label">{port.label}</span>
            {#if port.inUseBy !== null}
              <span class="in-use">{t('in use: %@', port.inUseBy)}</span>
            {/if}
          </label>
        </li>
      {/each}
    </ul>
  {/if}
  {#if adHoc}
    <div class="settings">
      <SerialSettingsFields bind:form idPrefix="serial-adhoc" {baudError} />
    </div>
  {/if}
  {#snippet footer()}
    <Button onclick={() => onanswer(null)}>{t('Cancel')}</Button>
    <Button variant="primary" disabled={!ready} onclick={connect}>{t('Connect')}</Button>
  {/snippet}
</Modal>

<style>
  .ports-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: var(--space-1);
    color: var(--fg-secondary);
  }
  .ports {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0 0 var(--space-3);
    padding: 0;
    list-style: none;
  }
  .port {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .path {
    font-family: var(--font-mono, monospace);
  }
  .label {
    color: var(--fg-secondary);
  }
  .in-use {
    margin-left: auto;
    color: var(--label-orange);
  }
  .empty {
    margin: 0 0 var(--space-3);
    color: var(--fg-secondary);
  }
  .settings {
    margin-top: var(--space-2);
  }
</style>
