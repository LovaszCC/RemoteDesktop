<script lang="ts">
  // Swift "Software Update" window as a sheet: checking, up to date, available (notes, Later / Install),
  // installing (progress), installed (Later / Restart now) and failed (Try again).
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import { i18n, t } from '../i18n/index.svelte';
  import { notesText, progressFraction, progressText } from './model';
  import { updates as defaultStore, type UpdateStore } from './updates.svelte';

  let { store = defaultStore }: { store?: UpdateStore } = $props();

  const phase = $derived(store.phase);
  const close = () => store.close();
</script>

{#if store.isOpen}
  <Modal title={t('Software Update')} onclose={close} width={480}>
    <div class="content" data-testid="update-dialog" data-phase={phase.kind}>
      {#if phase.kind === 'idle' || phase.kind === 'checking'}
        <p class="row"><span class="spinner" aria-hidden="true"></span>{t('Checking for updates…')}</p>
      {:else if phase.kind === 'notConfigured'}
        <p class="headline">{t("Updates aren't set up in this build.")}</p>
        <p class="secondary">{t('You have version %@.', phase.currentVersion)}</p>
      {:else if phase.kind === 'upToDate'}
        <p class="headline">{t('RemoteDesktop is up to date.')}</p>
        <p class="secondary">{t('You have version %@.', phase.currentVersion)}</p>
      {:else if phase.kind === 'available' || phase.kind === 'installing' || phase.kind === 'installed'}
        <p class="headline">{t('RemoteDesktop %@ is available.', phase.update.version)}</p>
        <p class="secondary">{t('You have version %@.', phase.update.currentVersion)}</p>
        {#if phase.kind === 'available' && notesText(phase.update.notes)}
          <pre class="notes selectable" data-testid="update-notes">{notesText(phase.update.notes)}</pre>
        {:else if phase.kind === 'installing'}
          <progress
            max="1"
            value={progressFraction(phase.downloaded, phase.total) ?? undefined}
            aria-label={t('Downloading…')}
          ></progress>
          <p class="secondary" data-testid="update-progress">
            {t('Downloading…')}
            {progressText(phase.downloaded, phase.total, i18n.locale)}
          </p>
        {:else if phase.kind === 'installed'}
          <p class="headline">{t('The update is installed.')}</p>
          <p>{t('Restart RemoteDesktop to use the new version.')}</p>
        {/if}
      {:else if phase.kind === 'failed'}
        <p class="row error"><TriangleAlert size={16} />{phase.message}</p>
      {/if}
    </div>
    {#snippet footer()}
      {#if phase.kind === 'available'}
        <Button onclick={close}>{t('Later')}</Button>
        <Button variant="primary" onclick={() => void store.install()}>{t('Install update')}</Button>
      {:else if phase.kind === 'installed'}
        <Button onclick={close}>{t('Later')}</Button>
        <Button variant="primary" onclick={() => void store.relaunch()}>{t('Restart now')}</Button>
      {:else if phase.kind === 'failed'}
        <Button onclick={close}>{t('Cancel')}</Button>
        <Button
          variant="primary"
          onclick={() => void (phase.update ? store.install() : store.checkManually())}
          >{t('Try again')}</Button
        >
      {:else if phase.kind === 'installing' || phase.kind === 'checking'}
        <Button onclick={close}>{t('Hide')}</Button>
      {:else}
        <Button variant="primary" onclick={close}>{t('OK')}</Button>
      {/if}
    {/snippet}
  </Modal>
{/if}

<style>
  .content p {
    margin: 0 0 var(--space-2);
  }
  .headline {
    font-weight: 600;
  }
  .secondary {
    color: var(--fg-secondary);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .error {
    align-items: flex-start;
    overflow-wrap: anywhere;
  }
  .notes {
    max-height: 180px;
    margin: var(--space-2) 0 0;
    padding: var(--space-2);
    overflow: auto;
    border-radius: var(--radius);
    background: var(--bg-control);
    font-family: inherit;
    font-size: var(--font-size-callout);
    white-space: pre-wrap;
  }
  progress {
    width: 100%;
    margin: var(--space-2) 0;
  }
  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid var(--fg-secondary);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
