<script lang="ts">
  import { t } from '../i18n/index.svelte';
  import { app } from '../stores/app.svelte';
  import ErrorView from './ErrorView.svelte';
  import LockConflictDialog from './LockConflictDialog.svelte';
  import MainView from './MainView.svelte';
  import UnlockView from './UnlockView.svelte';

  import { settingsWindow } from '../stores/settingsWindow.svelte';

  const openSettings = () => settingsWindow.open('general');
  const phase = $derived(app.phase);
</script>

{#if phase === null}
  <div class="loading">{t('Loading…')}</div>
{:else if phase.kind === 'locked'}
  {#key phase.needsSetup}
    <UnlockView needsSetup={phase.needsSetup} onotherdatabase={openSettings} />
  {/key}
{:else if phase.kind === 'unlocked'}
  <MainView />
{:else if phase.kind === 'noDatabase'}
  <ErrorView {phase} onsettings={openSettings} />
{:else if phase.kind === 'lockConflict'}
  <div class="backdrop-screen"></div>
  <LockConflictDialog holder={phase.holder} />
{/if}

<style>
  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--fg-tertiary);
  }
  .backdrop-screen {
    height: 100%;
    background: var(--bg-sidebar);
  }
</style>
