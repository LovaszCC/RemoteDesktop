<script lang="ts">
  import Folder from '@lucide/svelte/icons/folder';
  import Button from '../components/Button.svelte';
  import { labelColorVar } from '../colors';
  import { t, tn } from '../i18n/index.svelte';
  import { CONNECTION_TYPES, typeName } from '../sidebar/model';
  import type { FolderDto } from '../types';
  import { viewDeps } from './deps';
  import { directConnectionCount } from './model';

  /** Swift `FolderDetailView`: the name, "N connections · path", New <Type> and Rename. */
  let { folder }: { folder: FolderDto } = $props();
  const deps = viewDeps();
  const count = $derived(directConnectionCount(deps.library.library, folder.id));
</script>

<div class="centered">
  <span class="icon" style:color={folder.color ? labelColorVar(folder.color) : 'var(--accent)'}>
    <Folder size={40} strokeWidth={1.5} />
  </span>
  <h1 class="selectable">{folder.name}</h1>
  <p>{tn('%lld connections · %@', count, folder.path)}</p>
  <div class="buttons">
    {#each CONNECTION_TYPES as type (type)}
      <Button disabled={deps.readOnly} onclick={() => deps.actions.newConnection(type, folder.id)}>
        {t('New %@', typeName(type))}
      </Button>
    {/each}
    <Button disabled={deps.readOnly} onclick={() => deps.actions.renameFolder(folder.id)}
      >{t('Rename')}</Button
    >
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
  }
  h1 {
    margin: 0;
    font-size: var(--font-size-large-title);
    font-weight: 600;
  }
  p {
    margin: 0;
    color: var(--fg-secondary);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
</style>
