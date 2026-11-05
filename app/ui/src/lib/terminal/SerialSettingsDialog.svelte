<script lang="ts">
  // A serial tab's "Settings…": applied to the open port (the tab and its output stay); a saved
  // connection's tab can also save them to the connection.
  import Button from '../components/Button.svelte';
  import Checkbox from '../components/Checkbox.svelte';
  import Modal from '../components/Modal.svelte';
  import { t } from '../i18n/index.svelte';
  import type { SerialSettings } from '../types';
  import { serialForm, settingsFromForm, type SerialForm } from './serial';
  import SerialSettingsFields from './SerialSettingsFields.svelte';

  interface Props {
    settings: SerialSettings;
    canSave: boolean;
    onanswer: (answer: { settings: SerialSettings; save: boolean } | null) => void;
  }

  let { settings: initial, canSave, onanswer }: Props = $props();
  // Seeded once from the props: the dialog owns the fields while it is open.
  // svelte-ignore state_referenced_locally
  let form = $state<SerialForm>(serialForm(initial));
  let save = $state(false);
  const settings = $derived(settingsFromForm(form));
  const baudError = $derived(settings === null ? t('Enter a positive whole number.') : null);

  function apply() {
    if (settings) onanswer({ settings, save: canSave && save });
  }
</script>

<Modal title={t('Serial settings')} onclose={() => onanswer(null)} width={400}>
  <SerialSettingsFields bind:form idPrefix="serial-tab" {baudError} />
  {#if canSave}
    <Checkbox bind:checked={save} label={t('Save to the connection')} />
  {/if}
  {#snippet footer()}
    <Button onclick={() => onanswer(null)}>{t('Cancel')}</Button>
    <Button variant="primary" disabled={settings === null} onclick={apply}>{t('Apply')}</Button>
  {/snippet}
</Modal>
