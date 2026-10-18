<script lang="ts">
  // Shows a session's long-lived terminal (Swift `TerminalHostView`): attaches its element on mount and
  // detaches it on unmount without ending anything (unless it already moved to another view, e.g. a split
  // pane). Fits on every container resize and when shown.
  import { onMount, tick } from 'svelte';
  import type { TerminalHandle } from './xterm';

  let { handle, active }: { handle: TerminalHandle; active: boolean } = $props();
  let container: HTMLDivElement | undefined = $state();

  onMount(() => {
    // The terminal of this mount: the prop may be gone by the time it unmounts.
    const [host, terminal] = [container, handle];
    if (!host) return;
    terminal.attach(host);
    const observer = new ResizeObserver(() => terminal.fit());
    observer.observe(host);
    return () => {
      observer.disconnect();
      // A split may already have moved the terminal into its pane.
      terminal.detach(host);
    };
  });

  $effect(() => {
    if (!active) return;
    const terminal = handle;
    void tick().then(() => {
      terminal.fit();
      terminal.focus();
    });
  });
</script>

<div class="pane" bind:this={container}></div>

<style>
  .pane {
    flex: 1;
    min-height: 0;
    padding: 4px 0 0 6px;
    background: var(--terminal-bg);
  }
  .pane :global(.terminal-host) {
    width: 100%;
    height: 100%;
  }
</style>
