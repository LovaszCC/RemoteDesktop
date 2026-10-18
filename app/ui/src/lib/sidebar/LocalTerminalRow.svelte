<script lang="ts">
  // The fixed "Local Terminal" entry above the library: not part of the library (no selection, drag,
  // rename or delete). Double-click or Enter opens this computer's shell in a new tab; its context menu
  // has just "Open".
  import Terminal from '@lucide/svelte/icons/terminal';
  import { showContextMenu } from '../components/menu.svelte';
  import { t } from '../i18n/index.svelte';
  import type { SidebarActions } from './actions.svelte';
  import { localTerminalMenu } from './menus';

  let { actions }: { actions: Pick<SidebarActions, 'openLocalTerminal'> } = $props();

  const open = () => void actions.openLocalTerminal();
</script>

<div
  class="row"
  role="button"
  tabindex="0"
  data-local-terminal
  title={t('Opens a shell on this computer in a new tab')}
  onmousedown={(e) => {
    // Keep the double-click from selecting text.
    if (e.detail > 1) e.preventDefault();
  }}
  ondblclick={open}
  onkeydown={(e) => {
    if (e.key !== 'Enter' || e.altKey || e.metaKey || e.ctrlKey || e.shiftKey) return;
    // The list's own Enter would open the selected library row too.
    e.preventDefault();
    e.stopPropagation();
    open();
  }}
  oncontextmenu={(e) => {
    e.preventDefault();
    e.stopPropagation();
    showContextMenu(e, localTerminalMenu(actions));
  }}
>
  <span class="icon"><Terminal size={15} strokeWidth={1.75} /></span>
  <span class="name">{t('Local Terminal')}</span>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 26px;
    margin-top: 2px;
    padding: 2px 8px 2px 6px;
    border-radius: 7px;
    cursor: default;
    outline: none;
    user-select: none;
    transition: background-color 0.12s ease;
  }
  .row:hover {
    background: var(--chrome-hover);
  }
  .row:focus-visible {
    background: var(--selection-accent);
    color: #fff;
  }
  .icon {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 18px;
    color: var(--fg-secondary);
  }
  .row:focus-visible .icon {
    color: #fff;
  }
  .name {
    overflow: hidden;
    flex: 1;
    min-width: 0;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
