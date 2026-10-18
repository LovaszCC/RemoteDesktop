<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte';

  interface Props {
    /** Heading; also the dialog's accessible name. */
    title?: string;
    /** Esc, and a click on the backdrop when `dismissible`. */
    onclose?: () => void;
    dismissible?: boolean;
    width?: number;
    children: Snippet;
    /** Buttons, right-aligned. */
    footer?: Snippet;
    /** What gets the focus when it opens: the first text field (a form), or the sheet itself (a long
     * page like Settings, which would otherwise scroll to a field further down). */
    initialFocus?: 'field' | 'sheet';
  }

  let {
    title,
    onclose,
    dismissible = false,
    width = 420,
    children,
    footer,
    initialFocus = 'field',
  }: Props = $props();
  let sheet: HTMLDivElement | undefined = $state();
  const titleId = `modal-title-${Math.random().toString(36).slice(2)}`;

  const FOCUSABLE =
    'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  onMount(() => {
    const previous = document.activeElement as HTMLElement | null;
    void tick().then(() => {
      if (!sheet || sheet.contains(document.activeElement)) return;
      if (initialFocus === 'sheet') {
        sheet.focus();
        return;
      }
      const preferred = sheet.querySelector<HTMLElement>('[autofocus], input:not([disabled])');
      (preferred ?? sheet.querySelector<HTMLElement>(FOCUSABLE) ?? sheet).focus();
    });
    return () => previous?.focus?.();
  });

  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape' && onclose) {
      event.preventDefault();
      event.stopPropagation();
      onclose();
      return;
    }
    if (event.key !== 'Tab' || !sheet) return;
    // Keep focus inside the sheet.
    const items = [...sheet.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onpointerdown={(e) => {
    if (dismissible && e.target === e.currentTarget) onclose?.();
  }}
  onkeydown={keydown}
>
  <div
    bind:this={sheet}
    class="sheet"
    role="dialog"
    aria-modal="true"
    aria-labelledby={title ? titleId : undefined}
    tabindex="-1"
    style:width="{width}px"
  >
    {#if title}<h2 id={titleId}>{title}</h2>{/if}
    <div class="body">{@render children()}</div>
    {#if footer}<div class="footer">{@render footer()}</div>{/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 8vh;
    background: var(--bg-overlay);
    -webkit-backdrop-filter: blur(2px);
    backdrop-filter: blur(2px);
    animation: fade 0.14s ease-out;
  }
  .sheet {
    display: flex;
    flex-direction: column;
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 12vh - 16px);
    padding: var(--space-4) var(--space-5);
    border-radius: 14px;
    background: var(--glass-surface-strong);
    box-shadow: var(--glass-edge), var(--shadow-sheet);
    outline: none;
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    animation: drop 0.22s cubic-bezier(0.32, 0.72, 0, 1);
  }
  h2 {
    margin: 0 0 var(--space-3);
    font-size: var(--font-size);
    font-weight: 600;
  }
  .body {
    overflow: auto;
  }
  .footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    margin-top: var(--space-4);
  }
  @keyframes fade {
    from {
      opacity: 0;
    }
  }
  @keyframes drop {
    from {
      transform: translateY(-10px) scale(0.98);
      opacity: 0;
    }
  }
</style>
