<script lang="ts">
  import { untrack } from 'svelte';
  import Button from '../components/Button.svelte';
  import ColorPicker from '../components/ColorPicker.svelte';
  import Modal from '../components/Modal.svelte';
  import Select from '../components/Select.svelte';
  import TextField from '../components/TextField.svelte';
  import { viewDeps } from '../detail/deps';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import type { FolderEditorRequest } from '../stores/editor.svelte';
  import type { LabelColor } from '../types';
  import { canSave, folderOptions, fromPicker, toPicker } from './model';

  /** Swift `FolderEditorView`: name (Return saves), the parent for new folders, and the colour. */
  let { request, onclose }: { request: FolderEditorRequest; onclose: () => void } = $props();
  const deps = viewDeps();
  const fieldId = `folder-name-${Math.random().toString(36).slice(2)}`;

  const editedId = untrack(() => (request.mode === 'edit' ? request.id : null));
  const isNew = editedId === null;
  const existing = untrack(() => deps.library.folder(editedId));
  let name = $state(existing?.name ?? '');
  let color = $state<LabelColor | null>(existing?.color ?? null);
  let parentId = $state(untrack(() => (request.mode === 'new' ? toPicker(request.parentId) : '')));

  $effect(() => {
    if (editedId && !deps.library.folder(editedId)) onclose();
  });

  async function save() {
    if (!canSave(name) || deps.readOnly) return;
    const folder = isNew
      ? { parentId: fromPicker(parentId), name: name.trim(), color }
      : { id: editedId, name: name.trim(), color };
    onclose();
    try {
      await deps.api.saveFolder(folder);
    } catch (error) {
      deps.alerts.show(errorMessage(error));
    }
  }
</script>

<Modal title={isNew ? t('New folder') : t('Rename folder')} {onclose} width={340}>
  <div class="folder-editor">
    <TextField
      id={fieldId}
      bind:value={name}
      placeholder={t('Name')}
      aria-label={t('Name')}
      onsubmit={save}
      autofocus
    />
    {#if isNew}
      <label class="row">
        <span>{t('Parent')}</span>
        <Select bind:value={parentId} label={t('Parent')} options={folderOptions(deps.library.library)} />
      </label>
    {/if}
    <div class="row">
      <span>{t('Color')}</span>
      <ColorPicker bind:value={color} />
    </div>
  </div>
  {#snippet footer()}
    <Button onclick={onclose}>{t('Cancel')}</Button>
    <Button variant="primary" disabled={!canSave(name) || deps.readOnly} onclick={save}>
      {isNew ? t('Create') : t('Save')}
    </Button>
  {/snippet}
</Modal>

<style>
  .folder-editor {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }
</style>
