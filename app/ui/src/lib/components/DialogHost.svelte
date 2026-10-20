<script lang="ts">
  import { dialogs as defaultDialogs, type DialogStore } from '../stores/dialogs.svelte';
  import ConfirmDialog from './ConfirmDialog.svelte';
  import NameDialog from './NameDialog.svelte';

  let { dialogs = defaultDialogs }: { dialogs?: DialogStore } = $props();
</script>

{#if dialogs.current}
  {@const dialog = dialogs.current}
  {#key dialog}
    {#if dialog.kind === 'confirm'}
      <ConfirmDialog
        title={dialog.title}
        message={dialog.message}
        confirmLabel={dialog.confirmLabel}
        destructive={dialog.destructive}
        option={dialog.option}
        onconfirm={(checked) => dialogs.accept(checked)}
        oncancel={() => dialogs.cancel()}
      />
    {:else}
      <NameDialog
        title={dialog.title}
        label={dialog.label}
        initial={dialog.initial}
        confirmLabel={dialog.confirmLabel}
        onsubmit={(name) => dialogs.accept(name)}
        oncancel={() => dialogs.cancel()}
      />
    {/if}
  {/key}
{/if}
