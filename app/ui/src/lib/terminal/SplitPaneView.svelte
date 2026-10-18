<script lang="ts">
  // Swift `PaneView`: one terminal of a split. The header shows the connection colour, title, root warning,
  // the BROADCAST badge, the copy-next-output button, the broadcast toggle and Close; then the terminal and,
  // after a non-zero exit, the exit bar. Frame: green on the pane you type into, orange on the other
  // broadcasting panes.
  import CircleDot from '@lucide/svelte/icons/circle-dot';
  import CircleStop from '@lucide/svelte/icons/circle-stop';
  import RadioTower from '@lucide/svelte/icons/radio-tower';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import X from '@lucide/svelte/icons/x';
  import { t } from '../i18n/index.svelte';
  import ConnectionIcon from '../sidebar/ConnectionIcon.svelte';
  import type { LibraryStore } from '../stores/library.svelte';
  import type { SettingsStore } from '../stores/settings.svelte';
  import type { SessionsStore } from './sessions.svelte';
  import { exitText } from './model';
  import SessionExitBar from './SessionExitBar.svelte';
  import type { SplitPane } from './split';
  import type { SplitsStore } from './splits.svelte';
  import TerminalPane from './TerminalPane.svelte';

  interface Props {
    splits: SplitsStore;
    sessions: SessionsStore;
    pane: SplitPane;
    focused: boolean;
    broadcasting: boolean;
    /** The tab is shown and this is its focused pane. */
    active: boolean;
    library: Pick<LibraryStore, 'connection'>;
    settings: SettingsStore;
  }

  let { splits, sessions, pane, focused, broadcasting, active, library, settings }: Props = $props();

  const session = $derived(sessions.state(pane.id));
  const handle = $derived(sessions.handle(pane.id));
  const connection = $derived(library.connection(pane.connectionId));
  const running = $derived(session?.status.kind === 'running');
  const recording = $derived(session?.recording ?? 'idle');
  const recordHint = $derived(settings.shortcut('recordOutput')?.display ?? '');
  const reconnectHint = $derived(settings.shortcut('reconnect')?.display ?? null);
</script>

{#if session}
  <div
    class="pane"
    class:focused
    class:broadcasting
    role="group"
    aria-label={session.title}
    data-pane={pane.id}
    onfocusin={() => splits.focus(pane.id)}
    onpointerdown={() => splits.focus(pane.id)}
  >
    <div class="header">
      {#if connection?.color}<span class="color" style:background="var(--label-{connection.color})"
        ></span>{/if}
      <ConnectionIcon type={connection?.type ?? 'ssh'} size={13} />
      <span class="title" class:strong={focused}>{session.title}</span>
      {#if session.root}
        <span class="root" title={t('Root: commands run with full privileges')}
          ><TriangleAlert size={13} /></span
        >
      {/if}
      {#if broadcasting}<span class="badge">{t('BROADCAST')}</span>{/if}
      {#if recording !== 'idle'}
        <span class="red" role="status">{recording === 'armed' ? t('● Waiting…') : t('● Recording…')}</span>
      {/if}
      <span class="spacer"></span>
      <button
        type="button"
        class="icon"
        disabled={!running}
        aria-label={recording === 'idle' ? t('Copy next output') : t('Stop & copy')}
        title={t('Copies the output of the next command you enter to the clipboard (%@)', recordHint)}
        onclick={() => void sessions.toggleRecording(pane.id)}
      >
        {#if recording === 'idle'}<CircleDot size={13} />{:else}<span class="red"
            ><CircleStop size={13} /></span
          >{/if}
      </button>
      <button
        type="button"
        class="icon toggle"
        class:on={broadcasting}
        aria-pressed={broadcasting}
        aria-label={t('Broadcast')}
        title={t('Broadcast input: typing here also goes to the other broadcasting panes')}
        onclick={() => splits.toggleBroadcast(pane.id)}
      >
        <RadioTower size={13} />
      </button>
      <button
        type="button"
        class="icon"
        aria-label={t('Close this terminal')}
        title={t('Close this terminal')}
        onclick={() => splits.closePane(pane.id)}
      >
        <X size={12} strokeWidth={2.5} />
      </button>
    </div>
    {#if handle}<TerminalPane {handle} {active} />{/if}
    {#if session.status.kind === 'exited'}
      <SessionExitBar
        text={exitText(session.status)}
        {reconnectHint}
        onreconnect={() => void sessions.reconnect(pane.id)}
        onclose={() => splits.closePane(pane.id)}
      />
    {/if}
  </div>
{/if}

<style>
  .pane {
    position: relative;
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--terminal-bg);
  }
  .pane::after {
    position: absolute;
    inset: 0;
    border: 2px solid transparent;
    content: '';
    pointer-events: none;
  }
  .pane.broadcasting::after {
    border-color: var(--label-orange);
  }
  .pane.focused::after {
    border-color: var(--label-green);
  }
  .header {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
    padding: 3px 8px;
    background: var(--bg-toolbar);
    font-size: var(--font-size-callout);
  }
  .color {
    flex: none;
    width: 3px;
    height: 14px;
    border-radius: 1.5px;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .strong {
    font-weight: 600;
  }
  .root {
    display: inline-flex;
    color: var(--warning);
  }
  .badge {
    flex: none;
    padding: 0 5px;
    border-radius: 999px;
    background: var(--label-orange);
    color: white;
    font-size: 10px;
    font-weight: 700;
  }
  .red {
    display: inline-flex;
    color: var(--label-red);
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .icon {
    display: inline-flex;
    align-items: center;
    padding: 2px 4px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--fg-secondary);
  }
  .icon:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--fg);
  }
  .icon:disabled {
    opacity: 0.4;
  }
  .toggle.on {
    background: color-mix(in srgb, var(--label-orange) 25%, transparent);
    color: var(--label-orange);
  }
</style>
