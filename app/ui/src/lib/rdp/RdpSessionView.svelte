<script lang="ts">
  // Swift `RDPSessionView`: a small toolbar (Type clipboard, and while remote clipboard files download their
  // progress with Cancel), the remote desktop with a connecting / reconnecting overlay, and the exit bar with
  // Reconnect / Close once the session ended.
  //
  // The session's backing canvas (desktop-sized, painted by the store) is scaled to fill the view, so the
  // picture keeps up while the desktop resize is pending (smart sizing). The canvas takes the keyboard while
  // focused: every key goes to the session except the app shortcuts that work here (`rdpCommand`); a blur
  // releases every key and mouse button.
  import Keyboard from '@lucide/svelte/icons/keyboard';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import { onMount, tick } from 'svelte';
  import Button from '../components/Button.svelte';
  import { i18n, t } from '../i18n/index.svelte';
  import { rdpCommand } from '../shortcuts/dispatcher';
  import { app as defaultApp, type AppStore } from '../stores/app.svelte';
  import { settings as defaultSettings, type SettingsStore } from '../stores/settings.svelte';
  import SessionExitBar from '../terminal/SessionExitBar.svelte';
  import { key, KeyTracker, mouseButton, mouseMove, wheel } from './input';
  import { rdpExitText, transferBytes, transferFraction } from './model';
  import type { RdpSessionsStore } from './sessions.svelte';

  interface Props {
    sessions: RdpSessionsStore;
    id: string;
    active: boolean;
    settings?: SettingsStore;
    app?: Pick<AppStore, 'platform'>;
  }

  let { sessions, id, active, settings = defaultSettings, app = defaultApp }: Props = $props();

  const session = $derived(sessions.state(id));
  const status = $derived(session?.status);
  const transfer = $derived(session?.transfer ?? null);
  const fraction = $derived(transfer ? transferFraction(transfer) : null);
  const reconnectHint = $derived(settings.shortcut('reconnect')?.display ?? null);
  const typeHint = $derived(settings.shortcut('typeClipboard')?.display ?? '');
  let screen: HTMLDivElement | undefined = $state();
  let canvasHost: HTMLDivElement | undefined = $state();

  onMount(() => {
    const [host, holder] = [screen, canvasHost];
    const surface = sessions.surface(id);
    if (!host || !holder || !surface) return;
    const canvas = surface.element;
    canvas.tabIndex = 0;
    canvas.className = 'rdp-canvas';
    canvas.setAttribute('aria-label', t('Remote desktop'));
    holder.append(canvas);

    const keys = new KeyTracker();
    // Mouse buttons pressed on the canvas (their release counts even outside it).
    // eslint-disable-next-line svelte/prefer-svelte-reactivity
    const buttons = new Set<number>();
    /** X11 keyboard grab: keys reach the session directly, the canvas gets none. */
    let grabbed = false;
    const box = () => canvas.getBoundingClientRect();
    const send = (...events: Parameters<typeof sessions.input>[1]) => sessions.input(id, events);

    function releaseAll() {
      keys.clear();
      sessions.releaseAllKeys(id);
    }

    function onKeyDown(event: KeyboardEvent) {
      const command = rdpCommand(event, settings.shortcuts, app.platform);
      const connected = sessions.state(id)?.status.kind === 'connected';
      if (command && !(command === 'reconnect' && connected)) {
        // The window's dispatcher runs it; the session lets go of the modifiers already sent.
        releaseAll();
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      if (grabbed || event.isComposing) return;
      keys.press(event.code);
      send(key(event, true));
    }

    function onKeyUp(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();
      if (grabbed || !keys.release(event.code)) return;
      send(key(event, false));
    }

    function onMouseDown(event: MouseEvent) {
      event.preventDefault();
      canvas.focus();
      buttons.add(event.button);
      send(mouseButton(event, true, box()));
    }

    function onMouseUp(event: MouseEvent) {
      if (!buttons.delete(event.button)) return;
      send(mouseButton(event, false, box()));
    }

    function onMouseMove(event: MouseEvent) {
      if (event.target !== canvas && buttons.size === 0) return;
      send(mouseMove(event, box()));
    }

    function onWheel(event: WheelEvent) {
      event.preventDefault();
      send(wheel(event, box()));
    }

    function onFocus() {
      void sessions.captureKeyboard(id, true).then((grab) => {
        grabbed = grab && document.activeElement === canvas;
      });
    }

    function onBlur() {
      releaseAll();
      grabbed = false;
      const rect = box();
      for (const button of buttons) {
        send(mouseButton({ button, clientX: rect.left, clientY: rect.top }, false, rect));
      }
      buttons.clear();
      void sessions.captureKeyboard(id, false);
    }

    const preventMenu = (event: Event) => event.preventDefault();
    canvas.addEventListener('keydown', onKeyDown);
    canvas.addEventListener('keyup', onKeyUp);
    canvas.addEventListener('mousedown', onMouseDown);
    canvas.addEventListener('wheel', onWheel, { passive: false });
    canvas.addEventListener('contextmenu', preventMenu);
    canvas.addEventListener('focus', onFocus);
    canvas.addEventListener('blur', onBlur);
    window.addEventListener('mouseup', onMouseUp);
    window.addEventListener('mousemove', onMouseMove);

    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const { width, height } = entry.contentRect;
      sessions.resize(id, { width: Math.floor(width), height: Math.floor(height) });
    });
    observer.observe(host);

    return () => {
      observer.disconnect();
      canvas.removeEventListener('keydown', onKeyDown);
      canvas.removeEventListener('keyup', onKeyUp);
      canvas.removeEventListener('mousedown', onMouseDown);
      canvas.removeEventListener('wheel', onWheel);
      canvas.removeEventListener('contextmenu', preventMenu);
      canvas.removeEventListener('focus', onFocus);
      canvas.removeEventListener('blur', onBlur);
      window.removeEventListener('mouseup', onMouseUp);
      window.removeEventListener('mousemove', onMouseMove);
      if (document.activeElement === canvas) canvas.blur();
      canvas.remove();
    };
  });

  // The remote pointer is the canvas's own cursor.
  $effect(() => {
    const canvas = sessions.surface(id)?.element;
    if (canvas && session) canvas.style.cursor = session.cursor;
  });

  // The shown tab's canvas takes the focus; a hidden one gives it up.
  $effect(() => {
    const canvas = sessions.surface(id)?.element;
    if (!canvas) return;
    if (!active) {
      if (document.activeElement === canvas) canvas.blur();
      return;
    }
    void tick().then(() => canvas.focus());
  });
</script>

{#if session && status}
  <div class="rdp-session" data-session={id}>
    <div class="toolbar">
      <Button
        disabled={status.kind !== 'connected' || session.typing}
        title={t('Types the local clipboard text where the cursor is on the remote machine (%@)', typeHint)}
        onclick={() => void sessions.typeClipboard(id)}
      >
        <Keyboard size={13} />{session.typing ? t('Typing…') : t('Type clipboard')}
      </Button>
      {#if transfer}
        <div class="transfer" aria-live="polite">
          <span class="transfer-title">{t('Copying files from the remote computer…')}</span>
          <div
            class="track"
            role="progressbar"
            aria-label={t('Copying files from the remote computer…')}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={fraction === null ? undefined : Math.round(fraction * 100)}
          >
            <div
              class="fill"
              class:indeterminate={fraction === null}
              style:width="{(fraction ?? 0.3) * 100}%"
            ></div>
          </div>
          <span class="bytes">{transferBytes(transfer, i18n.locale)}</span>
          <Button onclick={() => sessions.cancelFileTransfer(id)}>{t('Cancel')}</Button>
        </div>
      {/if}
    </div>
    <div class="screen" data-rdp bind:this={screen}>
      <div class="canvas-host" bind:this={canvasHost}></div>
      {#if status.kind === 'connecting' || status.kind === 'reconnecting'}
        <div class="overlay" role="status">
          <span class="spin"><LoaderCircle size={16} /></span>
          {status.kind === 'connecting'
            ? t('Connecting: %@…', session.connectionName)
            : t('Reconnecting… (attempt %lld)', status.attempt)}
        </div>
      {/if}
    </div>
    {#if status.kind === 'disconnected'}
      <SessionExitBar
        text={rdpExitText(status)}
        {reconnectHint}
        onreconnect={() => void sessions.reconnect(id)}
        onclose={() => sessions.close(id)}
      />
    {/if}
  </div>
{/if}

<style>
  .rdp-session {
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
    padding: 4px 10px;
    border-bottom: 0.5px solid var(--separator);
    background: var(--bg-toolbar);
    font-size: var(--font-size-callout);
  }
  .toolbar :global(.button) {
    min-height: 20px;
    padding: 1px 8px;
    font-size: var(--font-size-callout);
  }
  .transfer {
    display: flex;
    flex: 1;
    align-items: center;
    justify-content: flex-end;
    gap: var(--space-2);
    min-width: 0;
  }
  .transfer-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .track {
    flex: none;
    overflow: hidden;
    width: 140px;
    height: 6px;
    border-radius: 3px;
    background: var(--bg-selected);
  }
  .fill {
    height: 100%;
    border-radius: 3px;
    background: var(--accent);
    transition: width 0.2s;
  }
  .indeterminate {
    animation: pulse 1.2s ease-in-out infinite alternate;
  }
  @keyframes pulse {
    from {
      margin-left: 0;
    }
    to {
      margin-left: 70%;
    }
  }
  .bytes {
    color: var(--fg-secondary);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .screen {
    position: relative;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    background: #000;
  }
  .canvas-host {
    position: absolute;
    inset: 0;
  }
  .canvas-host :global(.rdp-canvas) {
    display: block;
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    outline: none;
    cursor: default;
  }
  .overlay {
    display: flex;
    position: absolute;
    top: 50%;
    left: 50%;
    align-items: center;
    gap: var(--space-2);
    padding: 16px 20px;
    transform: translate(-50%, -50%);
    border-radius: 14px;
    background: var(--glass-surface);
    box-shadow: var(--glass-edge), var(--shadow-popover);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    color: var(--fg);
    pointer-events: none;
  }
  .spin {
    display: inline-flex;
    animation: spin 1s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
