<script lang="ts">
  import { viewDeps } from '../detail/deps';
  import CredentialEditor from './CredentialEditor.svelte';
  import EntryEditor from './EntryEditor.svelte';
  import FolderEditor from './FolderEditor.svelte';

  /** Renders the editor sheet the editor store asks for. */
  const { editor } = viewDeps();
</script>

{#if editor.entry}
  {#key editor.entry}
    <EntryEditor request={editor.entry} onclose={() => editor.closeEntryEditor()} />
  {/key}
{/if}
{#if editor.credential}
  {#key editor.credential}
    <CredentialEditor request={editor.credential} onclose={() => editor.closeCredentialEditor()} />
  {/key}
{/if}
{#if editor.folder}
  {#key editor.folder}
    <FolderEditor request={editor.folder} onclose={() => editor.closeFolderEditor()} />
  {/key}
{/if}
