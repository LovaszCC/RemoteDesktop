<script lang="ts">
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from '../components/Button.svelte';
  import { errorMessage, noDatabaseMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import { alerts } from '../stores/alert.svelte';
  import { app } from '../stores/app.svelte';
  import type { Phase } from '../types';

  interface Props {
    phase: Extract<Phase, { kind: 'noDatabase' }>;
    /** "Settings…". */
    onsettings: () => void;
  }

  let { phase, onsettings }: Props = $props();
  let busy = $state(false);

  async function retry() {
    busy = true;
    try {
      await app.reopenDatabase();
    } catch (error) {
      alerts.show(errorMessage(error));
    } finally {
      busy = false;
    }
  }
</script>

<div class="error-view">
  <TriangleAlert size={40} strokeWidth={1.25} />
  <h1>{t('Error')}</h1>
  <p class="message selectable">{noDatabaseMessage(phase)}</p>
  <div class="actions">
    <Button onclick={onsettings}>{t('Settings…')}</Button>
    <Button onclick={retry} disabled={busy}>{t('Retry')}</Button>
  </div>
</div>

<style>
  .error-view {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    height: 100%;
    padding: 40px;
    color: var(--fg-secondary);
    text-align: center;
  }
  h1 {
    margin: var(--space-2) 0 0;
    color: var(--fg);
    font-size: var(--font-size-title);
    font-weight: 600;
  }
  .message {
    max-width: 480px;
    margin: 0;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
</style>
