<script lang="ts">
  // The Settings sheet (Swift `SettingsView`, 520×560): "General" and "Keyboard Shortcuts".
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import Segmented from '../components/Segmented.svelte';
  import { t } from '../i18n/index.svelte';
  import { recorder } from '../shortcuts/recorder.svelte';
  import { settingsWindow, type SettingsTab } from '../stores/settingsWindow.svelte';
  import GeneralSettings from './GeneralSettings.svelte';
  import ShortcutsSettings from './ShortcutsSettings.svelte';

  const tabs = $derived<{ value: SettingsTab; label: string }[]>([
    { value: 'general', label: t('General') },
    { value: 'shortcuts', label: t('Keyboard Shortcuts') },
  ]);

  function close() {
    // Esc while recording only cancels the recording (the recorder swallows it first).
    if (!recorder.active) settingsWindow.close();
  }
</script>

<Modal title={t('Settings')} onclose={close} dismissible width={560} initialFocus="sheet">
  <div class="settings">
    <div class="tabs">
      <Segmented
        value={settingsWindow.tab}
        options={tabs}
        label={t('Settings')}
        onchange={(tab) => (settingsWindow.tab = tab)}
      />
    </div>
    <div class="page">
      {#if settingsWindow.tab === 'general'}
        <GeneralSettings />
      {:else}
        <ShortcutsSettings />
      {/if}
    </div>
  </div>
  {#snippet footer()}
    <Button variant="primary" onclick={() => settingsWindow.close()}>{t('Done')}</Button>
  {/snippet}
</Modal>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    height: min(560px, calc(100vh - 12vh - 120px));
  }
  .tabs {
    display: flex;
    justify-content: center;
  }
  .tabs :global(.segmented) {
    width: 300px;
  }
  .page {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding-right: 2px;
  }
</style>
