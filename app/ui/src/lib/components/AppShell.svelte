<script lang="ts">
  import type { Snippet } from 'svelte';
  import {
    SIDEBAR_COLLAPSE_AT,
    SIDEBAR_MIN,
    clampSidebarWidth,
    loadSidebarWidth,
    saveSidebarWidth,
  } from '../stores/layout';
  import { sidebarLayout as defaultLayout, type SidebarLayoutStore } from '../stores/sidebarLayout.svelte';

  interface Props {
    sidebar: Snippet;
    /** Above the main column's content: the toolbar row. */
    toolbar?: Snippet;
    /** Between the toolbar and the content (banners, the tab bar). */
    header?: Snippet;
    children: Snippet;
    /** Whether the sidebar is shown (tests pass their own). */
    layout?: Pick<SidebarLayoutStore, 'collapsed' | 'set'>;
  }

  let { sidebar, toolbar, header, children, layout = defaultLayout }: Props = $props();
  // On macOS the toolbar is the window's title bar (tauri.macos.conf.json): it and the strip above the
  // sidebar drag the window.
  const glass = typeof document !== 'undefined' && document.documentElement.dataset.glass !== undefined;
  let width = $state(loadSidebarWidth());
  let dragging = $state(false);
  const collapsed = $derived(layout.collapsed);

  function startResize(event: PointerEvent) {
    const handle = event.currentTarget as HTMLElement;
    handle.setPointerCapture(event.pointerId);
    const startX = event.clientX;
    const startWidth = collapsed ? 0 : width;
    dragging = true;
    // Dragged far enough below the minimum the sidebar hides (as in Finder); dragged out again it comes
    // back at its minimum or more.
    const move = (e: PointerEvent) => {
      const wanted = startWidth + e.clientX - startX;
      if (wanted < SIDEBAR_COLLAPSE_AT) {
        if (!layout.collapsed) layout.set(true);
        return;
      }
      if (layout.collapsed) layout.set(false);
      width = clampSidebarWidth(Math.max(SIDEBAR_MIN, wanted));
    };
    const end = () => {
      dragging = false;
      handle.removeEventListener('pointermove', move);
      handle.removeEventListener('pointerup', end);
      handle.removeEventListener('pointercancel', end);
      saveSidebarWidth(width);
    };
    handle.addEventListener('pointermove', move);
    handle.addEventListener('pointerup', end);
    handle.addEventListener('pointercancel', end);
  }

  function keyResize(event: KeyboardEvent) {
    const step = event.shiftKey ? 40 : 10;
    if (event.key === 'ArrowRight' && collapsed) layout.set(false);
    else if (event.key === 'ArrowLeft') width = clampSidebarWidth(width - step);
    else if (event.key === 'ArrowRight') width = clampSidebarWidth(width + step);
    else return;
    event.preventDefault();
    saveSidebarWidth(width);
  }
</script>

<div class="shell" class:dragging class:collapsed style:--sidebar-width="{collapsed ? 0 : width}px">
  <aside class="sidebar" inert={collapsed} aria-hidden={collapsed}>
    {#if glass}<div class="titlebar-strip" data-tauri-drag-region></div>{/if}
    {@render sidebar()}
  </aside>
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="resizer"
    role="separator"
    aria-orientation="vertical"
    aria-valuenow={collapsed ? 0 : width}
    tabindex="0"
    onpointerdown={startResize}
    onkeydown={keyResize}
  ></div>
  <section class="main">
    {#if toolbar}
      <div class="toolbar" data-tauri-drag-region={glass || undefined}>{@render toolbar()}</div>
    {/if}
    <div class="panel">
      {#if header}{@render header()}{/if}
      <div class="content">{@render children()}</div>
    </div>
  </section>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: var(--sidebar-width) 0 minmax(0, 1fr);
    height: 100%;
    transition: grid-template-columns 0.22s cubic-bezier(0.32, 0.72, 0, 1);
  }
  .dragging {
    cursor: col-resize;
    transition: none;
  }
  @media (prefers-reduced-motion: reduce) {
    .shell {
      transition: none;
    }
  }
  .sidebar {
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
    background: var(--bg-sidebar);
  }
  .titlebar-strip {
    flex: none;
    height: var(--toolbar-height);
  }
  .resizer {
    position: relative;
    z-index: 5;
    width: 0;
  }
  .resizer::after {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: -3px;
    width: 6px;
    cursor: col-resize;
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    /* Keeps the grid row at the window height: otherwise tall content (two stacked terminals of a split)
       grows the row past the window and the bottom panes end up out of view. */
    min-height: 0;
  }
  .toolbar {
    display: flex;
    flex: none;
    align-items: center;
    gap: var(--space-1);
    height: var(--toolbar-height);
    padding: 0 calc(var(--panel-gap) + var(--space-1)) 0 var(--space-2);
    transition: padding-left 0.22s cubic-bezier(0.32, 0.72, 0, 1);
  }
  /* With the sidebar hidden the toolbar starts right of the macOS window buttons. */
  .collapsed .toolbar {
    padding-left: max(var(--space-2), var(--titlebar-inset));
  }
  .panel {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    margin: 0 var(--panel-gap) var(--panel-gap) 0;
    overflow: hidden;
    border-radius: var(--panel-radius);
    background: var(--bg-content);
    box-shadow: var(--shadow-panel);
    transition: margin-left 0.22s cubic-bezier(0.32, 0.72, 0, 1);
  }
  .collapsed .panel {
    margin-left: var(--panel-gap);
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
</style>
