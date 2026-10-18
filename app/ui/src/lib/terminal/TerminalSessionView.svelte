<script lang="ts">
  // Swift `TerminalSessionView`: a small toolbar (copy next output, its status, the root warning), the
  // terminal, and the "connection closed" bar with Reconnect / Close while the session has exited.
  import CircleDot from '@lucide/svelte/icons/circle-dot';
  import CircleStop from '@lucide/svelte/icons/circle-stop';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from '../components/Button.svelte';
  import { t } from '../i18n/index.svelte';
  import { settings as defaultSettings, type SettingsStore } from '../stores/settings.svelte';
  import { exitText } from './model';
  import SessionExitBar from './SessionExitBar.svelte';
  import type { SessionsStore } from './sessions.svelte';
  import TerminalPane from './TerminalPane.svelte';

  interface Props {
    sessions: SessionsStore;
    id: string;
    active: boolean;
    settings?: SettingsStore;
  }

  let { sessions, id, active, settings = defaultSettings }: Props = $props();

  const session = $derived(sessions.state(id));
  const handle = $derived(sessions.handle(id));
  const running = $derived(session?.status.kind === 'running');
  const idle = $derived((session?.recording ?? 'idle') === 'idle');
  const recordHint = $derived(settings.shortcut('recordOutput')?.display ?? '');
  const reconnectHint = $derived(settings.shortcut('reconnect')?.display ?? null);
</script>

{#if session}
  <div class="session" data-session={id}>
    <div class="toolbar">
      <Button
        disabled={!running}
        title={t('Copies the output of the next command you enter to the clipboard (%@)', recordHint)}
        onclick={() => void sessions.toggleRecording(id)}
      >
        {#if idle}
          <CircleDot size={13} />{t('Copy next output')}
        {:else}
          <span class="red"><CircleStop size={13} /></span>{t('Stop & copy')}
        {/if}
      </Button>
      {#if session.recording === 'armed'}
        <span class="red" role="status">{t('● Waiting for a command… (Esc: cancel)')}</span>
        <Button onclick={() => void sessions.cancelRecording(id)}>{t('Cancel')}</Button>
      {:else if session.recording === 'capturing'}
        <span class="red" role="status">{t('● Recording…')}</span>
        <Button onclick={() => void sessions.cancelRecording(id)}>{t('Cancel')}</Button>
      {/if}
      <span class="spacer"></span>
      {#if session.root}
        <span class="root" role="status">
          <TriangleAlert size={13} />{t('Root: commands run with full privileges')}
        </span>
      {/if}
    </div>
    {#if handle}<TerminalPane {handle} {active} />{/if}
    {#if session.status.kind === 'exited'}
      <SessionExitBar
        text={exitText(session.status)}
        {reconnectHint}
        onreconnect={() => void sessions.reconnect(id)}
        onclose={() => sessions.close(id)}
      />
    {/if}
  </div>
{/if}

<style>
  .session {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .toolbar {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--space-2);
    background: var(--bg-toolbar);
  }
  .toolbar {
    padding: 4px 10px;
    border-bottom: 0.5px solid var(--separator);
    font-size: var(--font-size-callout);
  }
  .toolbar :global(.button) {
    min-height: 20px;
    padding: 1px 8px;
    font-size: var(--font-size-callout);
  }
  .spacer {
    flex: 1;
  }
  .red {
    display: inline-flex;
    color: var(--label-red);
  }
  .root {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--warning);
    font-weight: 600;
  }
</style>
