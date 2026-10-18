<script lang="ts">
  import ContextMenu from './ContextMenu.svelte';
  import { closeContextMenu, contextMenu } from './menu.svelte';
</script>

{#if contextMenu.open}
  {#key contextMenu.open}
    <ContextMenu
      items={contextMenu.open.items}
      x={contextMenu.open.x}
      y={contextMenu.open.y}
      onclose={closeContextMenu}
    />
  {/key}
{/if}
