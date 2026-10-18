<script lang="ts">
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import { errorMessage } from '../errors';
  import { relativeAge } from '../format';
  import { i18n, t } from '../i18n/index.svelte';
  import { alerts } from '../stores/alert.svelte';
  import { app } from '../stores/app.svelte';
  import type { LockChoice, LockHolderDto } from '../types';

  let { holder }: { holder: LockHolderDto } = $props();
  let busy = $state(false);

  async function choose(choice: LockChoice) {
    busy = true;
    try {
      await app.resolveDbLock(choice);
    } catch (error) {
      alerts.show(errorMessage(error));
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={t('The database is in use')} onclose={() => void choose('cancel')} width={440}>
  <p>
    {t(
      'Another computer (%@) has this database open. It was last active %@.',
      holder.host,
      relativeAge(holder.ageSeconds, i18n.locale),
    )}
  </p>
  <p class="hint">{t('Opening it on two computers at once can overwrite changes. Read-only is safe.')}</p>
  {#snippet footer()}
    <Button onclick={() => choose('cancel')} disabled={busy}>{t('Cancel')}</Button>
    <Button onclick={() => choose('openAnyway')} disabled={busy}>{t('Open anyway')}</Button>
    <Button variant="primary" onclick={() => choose('readOnly')} disabled={busy} autofocus>
      {t('Open read-only')}
    </Button>
  {/snippet}
</Modal>

<style>
  p {
    margin: 0 0 var(--space-2);
  }
  .hint {
    margin: 0;
    color: var(--fg-secondary);
    font-size: var(--font-size-callout);
  }
</style>
