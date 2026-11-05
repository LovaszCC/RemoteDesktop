<script lang="ts">
  // Hosts the serial dialogs of `serialConsoles`: the port picker and a tab's "Settings…".
  import SerialConnectDialog from './SerialConnectDialog.svelte';
  import { serialConsoles as defaultStore, type SerialConsolesStore } from './serialConsoles.svelte';
  import SerialSettingsDialog from './SerialSettingsDialog.svelte';

  let { store = defaultStore }: { store?: SerialConsolesStore } = $props();
  const dialog = $derived(store.dialog);
</script>

{#if dialog?.kind === 'connect'}
  {#key dialog.resolve}
    <SerialConnectDialog
      adHoc={dialog.adHoc}
      name={dialog.name}
      ports={dialog.ports}
      onrefresh={() => void store.refresh()}
      onanswer={(answer) => store.answer(answer)}
    />
  {/key}
{:else if dialog?.kind === 'settings'}
  {#key dialog.resolve}
    <SerialSettingsDialog
      settings={dialog.settings}
      canSave={dialog.canSave}
      onanswer={(answer) => store.answer(answer)}
    />
  {/key}
{/if}
