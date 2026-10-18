<script lang="ts">
  // Swift `SessionExitedBar`: "Connection closed (…)" with Reconnect and Close, below an exited terminal
  // (a tab's or a split pane's) or an ended RDP session.
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Unplug from '@lucide/svelte/icons/unplug';
  import Button from '../components/Button.svelte';
  import { t } from '../i18n/index.svelte';

  interface Props {
    /** Why the session ended. */
    text: string;
    reconnectHint: string | null;
    onreconnect: () => void;
    onclose: () => void;
  }

  let { text, reconnectHint, onreconnect, onclose }: Props = $props();
</script>

<div class="exit-bar" role="alert">
  <span class="exit-icon"><Unplug size={15} /></span>
  <span class="exit-text">{text}</span>
  <span class="spacer"></span>
  <Button title={reconnectHint ? `${t('Reconnect')} (${reconnectHint})` : undefined} onclick={onreconnect}>
    <RotateCw size={13} />{t('Reconnect')}
  </Button>
  <Button onclick={onclose}>{t('Close')}</Button>
</div>

<style>
  .exit-bar {
    display: flex;
    flex: none;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-top: 0.5px solid var(--separator);
    background: var(--bg-toolbar);
  }
  .exit-icon {
    display: inline-flex;
    color: var(--label-orange);
  }
  .exit-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
</style>
