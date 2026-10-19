<script lang="ts">
  import MousePointerClick from '@lucide/svelte/icons/mouse-pointer-click';
  import { t } from '../i18n/index.svelte';
  import { isEntryKind, parseTag } from '../stores/library.svelte';
  import CredentialDetail from './CredentialDetail.svelte';
  import { viewDeps } from './deps';
  import EntryDetail from './EntryDetail.svelte';
  import FolderDetail from './FolderDetail.svelte';
  import MultiSelectionDetail from './MultiSelectionDetail.svelte';
  import SavedSplitDetail from './SavedSplitDetail.svelte';

  /** Swift `DetailView`: the selected item's view, the multi-selection view, or a calm placeholder. */
  const deps = viewDeps();
  const store = deps.library;

  const parsed = $derived(store.single ? parseTag(store.single) : null);
  // An entry row and a child row both show the entry; a child row highlights its connection.
  const selectedConnection = $derived(
    parsed?.kind === 'connection' ? store.connection(parsed.id) : undefined,
  );
  const entry = $derived(
    isEntryKind(parsed?.kind) ? store.entry(parsed?.id) : store.entry(selectedConnection?.entryId),
  );
  const connections = $derived(store.connectionsOf(entry?.id));
  const folder = $derived(parsed?.kind === 'folder' ? store.folder(parsed.id) : undefined);
  const credential = $derived(parsed?.kind === 'credential' ? store.credential(parsed.id) : undefined);
  const split = $derived(parsed?.kind === 'split' ? store.split(parsed.id) : undefined);
  const connectShortcut = $derived(deps.settings.shortcut('connect')?.display ?? null);
</script>

<div class="detail-view">
  {#if entry && connections.length > 0}
    <EntryDetail {entry} {connections} selectedId={selectedConnection?.id ?? null} />
  {:else if folder}
    <FolderDetail {folder} />
  {:else if credential}
    {#key credential.id}
      <CredentialDetail {credential} />
    {/key}
  {:else if split}
    <SavedSplitDetail {split} />
  {:else if store.selection.length > 1}
    <MultiSelectionDetail />
  {:else}
    <div class="placeholder">
      <span class="badge"><MousePointerClick size={28} strokeWidth={1.5} /></span>
      <h1>{t('Select a connection')}</h1>
      {#if connectShortcut}
        <p>{t('Double-click or %@ to connect', connectShortcut)}</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .detail-view {
    height: 100%;
  }
  .placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    height: 100%;
    color: var(--fg-tertiary);
  }
  .badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 64px;
    height: 64px;
    margin-bottom: var(--space-2);
    border-radius: 18px;
    background: linear-gradient(145deg, var(--accent-soft), transparent);
    box-shadow:
      inset 0 0.5px 0 rgba(255, 255, 255, 0.12),
      0 0 0 0.5px var(--accent-soft);
    color: var(--accent);
  }
  h1 {
    margin: 0;
    font-size: var(--font-size-title);
    font-weight: 600;
    color: var(--fg-secondary);
  }
  p {
    margin: 0;
  }
</style>
