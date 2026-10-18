<script lang="ts">
  import Globe from '@lucide/svelte/icons/globe';
  import Monitor from '@lucide/svelte/icons/monitor';
  import Network from '@lucide/svelte/icons/network';
  import SquareTerminal from '@lucide/svelte/icons/square-terminal';
  import type { ConnectionType } from '../types';

  /** Swift `ConnectionIcon`: the type's symbol in its tint (RDP blue, SSH green, web purple, MikroTik orange). */
  let { type, size = 15 }: { type: ConnectionType; size?: number } = $props();

  const ICONS = { rdp: Monitor, ssh: SquareTerminal, web: Globe, mikrotik: Network };
  const Icon = $derived(ICONS[type]);
</script>

<span class="connection-icon {type}"><Icon {size} strokeWidth={1.75} /></span>

<style>
  .connection-icon {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 18px;
  }
  .rdp {
    color: var(--label-blue);
  }
  .ssh {
    color: var(--label-green);
  }
  .web {
    color: var(--label-purple);
  }
  .mikrotik {
    color: var(--label-orange);
  }
</style>
