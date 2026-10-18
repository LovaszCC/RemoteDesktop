<script lang="ts">
  import Grid2x2 from '@lucide/svelte/icons/grid-2x2';
  import Button from '../components/Button.svelte';
  import Select from '../components/Select.svelte';
  import { credentialOptions, fromPicker, NONE } from '../editors/model';
  import { t } from '../i18n/index.svelte';
  import { MAX_PANES } from '../sidebar/actions.svelte';
  import { connectionIdsForTags, sshConnections } from '../sidebar/model';
  import { viewDeps } from './deps';

  /** Swift `MultipleSelectionView`: open the SSH ones side by side, or link them all to a credential. */
  const deps = viewDeps();
  let credentialId = $state(NONE);

  const tags = $derived(deps.library.selection);
  const connections = $derived(sshConnections(deps.library.library, tags));
  const linkable = $derived(connectionIdsForTags(deps.library.library, tags));
</script>

<div class="centered">
  <span class="icon"><Grid2x2 size={40} strokeWidth={1.5} /></span>
  <h1>{t('%lld items selected', tags.length)}</h1>
  <p>
    {connections.length < 2
      ? t('Select 2–%lld SSH or MikroTik connections to open them side by side.', MAX_PANES)
      : connections.map((c) => c.name).join(', ')}
  </p>
  <Button
    disabled={connections.length < 2}
    title={deps.settings.shortcut('connect')?.display ?? undefined}
    onclick={() => deps.actions.openSplit(connections)}
  >
    {t('Open in split view')}
  </Button>
  <div class="link">
    <Select
      bind:value={credentialId}
      label={t('Linked credential')}
      options={credentialOptions(deps.library.library, false)}
      disabled={deps.readOnly || linkable.length === 0}
    />
    <Button
      disabled={deps.readOnly || linkable.length === 0}
      onclick={() => deps.actions.setCredential(fromPicker(credentialId), tags)}
    >
      {t('Apply to %lld connections', linkable.length)}
    </Button>
  </div>
</div>

<style>
  .centered {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    min-height: 100%;
    padding: var(--space-5);
    text-align: center;
  }
  .icon {
    display: inline-flex;
    color: var(--fg-secondary);
  }
  h1 {
    margin: 0;
    font-size: var(--font-size-large-title);
    font-weight: 600;
  }
  p {
    max-width: 480px;
    margin: 0 0 var(--space-2);
    color: var(--fg-secondary);
  }
  .link {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
</style>
