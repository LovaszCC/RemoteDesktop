<script lang="ts">
  // The database was used by a newer RemoteDesktop: it stays closed until this app is updated. The update
  // dialog opens at once; Retry re-runs the open (e.g. after updating elsewhere), Quit leaves.
  import CircleArrowUp from '@lucide/svelte/icons/circle-arrow-up';
  import { onMount } from 'svelte';
  import { api as defaultApi, type Api } from '../api';
  import Button from '../components/Button.svelte';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
  import { app as defaultApp, type AppStore } from '../stores/app.svelte';
  import type { Phase } from '../types';
  import { updates as defaultUpdates, type UpdateStore } from '../updates/updates.svelte';

  interface Props {
    phase: Extract<Phase, { kind: 'needsUpdate' }>;
    api?: Pick<Api, 'appQuit'>;
    app?: Pick<AppStore, 'reopenDatabase'>;
    updates?: Pick<UpdateStore, 'checkManually'>;
    alerts?: Pick<AlertStore, 'show'>;
  }

  let {
    phase,
    api = defaultApi,
    app = defaultApp,
    updates = defaultUpdates,
    alerts = defaultAlerts,
  }: Props = $props();
  let busy = $state(false);

  const check = () => void updates.checkManually();
  onMount(check);

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

<div class="needs-update">
  <CircleArrowUp size={40} strokeWidth={1.25} />
  <h1>{t('Update required')}</h1>
  <p class="message">
    {t(
      'This database was last used with RemoteDesktop %@. This is version %@: update the app to open it.',
      phase.databaseVersion,
      phase.appVersion,
    )}
  </p>
  <div class="actions">
    <Button variant="primary" onclick={check}>{t('Check for updates')}</Button>
    <Button onclick={retry} disabled={busy}>{t('Retry')}</Button>
    <Button onclick={() => void api.appQuit()}>{t('Quit')}</Button>
  </div>
</div>

<style>
  .needs-update {
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
  }
  .actions {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
</style>
