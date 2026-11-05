<script lang="ts">
  // The fixed "Serial Console" entry under "Local Terminal": not part of the library. Double-click or Enter
  // opens the ad-hoc console dialog (port and settings typed by hand, nothing saved); its context menu has
  // just "Open".
  import Usb from '@lucide/svelte/icons/usb';
  import { showContextMenu } from '../components/menu.svelte';
  import { t } from '../i18n/index.svelte';
  import type { SidebarActions } from './actions.svelte';
  import { serialConsoleMenu } from './menus';

  let {
    actions,
    sessionOpen = false,
  }: {
    actions: Pick<SidebarActions, 'openSerialConsole'>;
    /** An ad-hoc console is open: green dot. */
    sessionOpen?: boolean;
  } = $props();

  const open = () => void actions.openSerialConsole();
</script>

<div
  class="row"
  role="button"
  tabindex="0"
  data-serial-console
  title={t('Opens a console on a USB serial adapter in a new tab')}
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
    showContextMenu(e, serialConsoleMenu(actions));
  }}
>
  <span class="icon"><Usb size={15} strokeWidth={1.75} /></span>
  <span class="name">{t('Serial Console')}</span>
  {#if sessionOpen}
    <span class="session-dot" title={t('Open session')} aria-label={t('Open session')} role="img"></span>
  {/if}
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
  .session-dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--label-green);
  }
  .name {
    overflow: hidden;
    flex: 1;
    min-width: 0;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
