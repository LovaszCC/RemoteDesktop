<script lang="ts">
  // Swift `PortForwardSection` in the SSH connection detail: the saved forwards with their tunnel state
  // (grey ring stopped, spinner starting, green dot active, red warning failed with the error), Copy
  // address while running, Start / Stop, Edit / Delete (buttons and context menu), and New forward.
  import Copy from '@lucide/svelte/icons/copy';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Play from '@lucide/svelte/icons/play';
  import Plus from '@lucide/svelte/icons/plus';
  import Square from '@lucide/svelte/icons/square';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from '../components/Button.svelte';
  import IconButton from '../components/IconButton.svelte';
  import { showContextMenu } from '../components/menu.svelte';
  import { viewDeps } from '../detail/deps';
  import { forwardDestination, forwardName, forwardsOf } from '../detail/model';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import type { ConnectionDto, PortForwardDraft, PortForwardDto } from '../types';
  import { isRunning } from './forwards.svelte';
  import { draftOf, failureText } from './model';
  import PortForwardEditor from './PortForwardEditor.svelte';

  let {
    connection,
    title = t('Port forwards'),
  }: {
    connection: ConnectionDto;
    /** The heading; an entry with several SSH connections names the connection in it. */
    title?: string;
  } = $props();
  const deps = viewDeps();
  const forwards = $derived(forwardsOf(deps.library.library, connection.id));
  let editing = $state<PortForwardDraft | null>(null);

  const report = (error: unknown) => deps.alerts.show(errorMessage(error));

  async function copy(address: string) {
    try {
      await deps.api.copyText(address);
      deps.toast.show(t('Copied'));
    } catch (error) {
      report(error);
    }
  }

  function remove(forward: PortForwardDto) {
    void deps.forwards.remove(forward.id).catch(report);
  }

  function menu(event: MouseEvent, forward: PortForwardDto) {
    showContextMenu(event, [
      {
        label: t('Edit…'),
        disabled: deps.readOnly,
        action: () => (editing = draftOf(connection.id, forward)),
      },
      { label: t('Delete'), destructive: true, disabled: deps.readOnly, action: () => remove(forward) },
    ]);
  }
</script>

<section>
  <div class="section-head">
    <h2>{title}</h2>
    <Button disabled={deps.readOnly} onclick={() => (editing = draftOf(connection.id))}>
      <Plus size={13} />{t('New forward')}
    </Button>
  </div>
  {#if forwards.length === 0}
    <p class="caption">
      {t(
        'Reach machines behind this SSH server: a port on this Mac is forwarded to a host and port the server can reach.',
      )}
    </p>
  {/if}
  {#each forwards as forward (forward.id)}
    {@const status = deps.forwards.status(forward.id)}
    {@const address = `localhost:${forward.localPort}`}
    {@const failure = status ? failureText(forward, status) : ''}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="forward" data-forward={forward.id} oncontextmenu={(e) => menu(e, forward)}>
      <span class="indicator">
        {#if status?.state === 'active'}
          <span class="dot" role="img" aria-label={t('Active')} title={t('Active')}></span>
        {:else if status?.state === 'starting'}
          <span class="spinner" role="img" aria-label={t('Starting…')} title={t('Starting…')}></span>
        {:else if status?.state === 'failed'}
          <span class="failed" role="img" aria-label={t('Failed')} title={status.error}>
            <TriangleAlert size={14} strokeWidth={2} />
          </span>
        {:else}
          <span class="ring" role="img" aria-label={t('Stopped')} title={t('Stopped')}></span>
        {/if}
      </span>
      <span class="text">
        <span class="name">{forwardName(forward)}</span>
        <span class="muted selectable">{address} → {forwardDestination(forward)}</span>
        {#if failure}<span class="error selectable" title={status?.state === 'failed' ? status.error : ''}
            >{failure}</span
          >{/if}
      </span>
      <span class="buttons">
        {#if isRunning(status)}
          <IconButton icon={Copy} label={t('Copy address')} onclick={() => void copy(address)} />
          <Button onclick={() => void deps.forwards.stopForward(forward.id).catch(report)}>
            <Square size={11} fill="currentColor" />{t('Stop')}
          </Button>
        {:else}
          <Button onclick={() => void deps.forwards.startForward(forward.id).catch(report)}>
            <Play size={12} fill="currentColor" />{t('Start')}
          </Button>
        {/if}
        <IconButton
          icon={Pencil}
          label={t('Edit…')}
          disabled={deps.readOnly}
          onclick={() => (editing = draftOf(connection.id, forward))}
        />
        <IconButton
          icon={Trash2}
          label={t('Delete')}
          disabled={deps.readOnly}
          onclick={() => remove(forward)}
        />
      </span>
    </div>
  {/each}
</section>

{#if editing}
  <PortForwardEditor
    draft={editing}
    forwards={deps.forwards}
    onclose={() => (editing = null)}
    onstarterror={report}
  />
{/if}

<style>
  .forward {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: var(--radius);
    background: var(--bg-hover);
  }
  .indicator {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 16px;
  }
  .dot,
  .ring {
    width: 9px;
    height: 9px;
    border-radius: 50%;
  }
  .dot {
    background: var(--label-green);
  }
  .ring {
    border: 1.5px solid var(--fg-secondary);
  }
  .failed {
    display: inline-flex;
    color: var(--danger);
  }
  .spinner {
    width: 10px;
    height: 10px;
    border: 2px solid var(--fg-secondary);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-weight: 500;
  }
  .error {
    color: var(--danger);
    font-size: var(--font-size-small);
    overflow-wrap: anywhere;
  }
  .buttons {
    display: flex;
    flex: none;
    align-items: center;
    gap: 4px;
  }
</style>
