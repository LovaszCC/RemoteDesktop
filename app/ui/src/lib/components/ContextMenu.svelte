<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import { onMount, tick } from 'svelte';
  import ContextMenu from './ContextMenu.svelte';
  import { isSeparator, type MenuItem } from './menu.svelte';

  interface Props {
    items: MenuItem[];
    x: number;
    y: number;
    /** Closes the whole menu (after an action, Esc at the root, or a click outside). */
    onclose: () => void;
    /** Set on submenus: closes only this level (← or Esc) and returns focus to the parent row. */
    onback?: () => void;
    /** The parent row's width, so a submenu that must flip left clears it. */
    parentWidth?: number;
  }

  let { items, x, y, onclose, onback, parentWidth = 0 }: Props = $props();

  let menu: HTMLDivElement | undefined = $state();
  let left = $state(0);
  let top = $state(0);
  let active = $state(-1);
  let submenuIndex = $state(-1);
  let submenuAt = $state({ x: 0, y: 0, width: 0 });
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;
  const nested = $derived(onback !== undefined);

  const enabled = (i: number) => {
    const item = items[i];
    return item !== undefined && !isSeparator(item) && !item.disabled;
  };

  onMount(() => {
    left = x;
    top = y;
    void tick().then(() => {
      if (!menu) return;
      // Keep the menu on screen: flip left/up when it would overflow.
      const rect = menu.getBoundingClientRect();
      const margin = 4;
      if (x + rect.width > window.innerWidth - margin) {
        left = nested ? Math.max(margin, x - parentWidth - rect.width) : Math.max(margin, x - rect.width);
      }
      if (y + rect.height > window.innerHeight - margin) {
        top = Math.max(margin, window.innerHeight - margin - rect.height);
      }
      menu.focus();
      if (nested) move(1);
    });
    if (nested) return;
    const outside = (event: PointerEvent) => {
      if (!(event.target instanceof Element) || !event.target.closest('.context-menu')) onclose();
    };
    const blur = () => onclose();
    window.addEventListener('pointerdown', outside, true);
    window.addEventListener('blur', blur);
    window.addEventListener('resize', blur);
    return () => {
      window.removeEventListener('pointerdown', outside, true);
      window.removeEventListener('blur', blur);
      window.removeEventListener('resize', blur);
      clearTimeout(hoverTimer);
    };
  });

  function move(step: number) {
    if (!items.some((_, i) => enabled(i))) return;
    let i = active;
    do {
      i = (i + step + items.length) % items.length;
    } while (!enabled(i));
    active = i;
  }

  async function openSubmenu(index: number, focus: boolean) {
    const row = menu?.querySelectorAll<HTMLElement>('[data-index]')[index];
    const item = items[index];
    if (!row || !item || isSeparator(item) || !item.submenu) return;
    const rect = row.getBoundingClientRect();
    submenuAt = { x: rect.right - 2, y: rect.top - 5, width: rect.width };
    submenuIndex = index;
    active = index;
    if (!focus) {
      await tick();
      menu?.focus();
    }
  }

  function activate(index: number) {
    const item = items[index];
    if (!item || isSeparator(item) || item.disabled) return;
    if (item.submenu) {
      void openSubmenu(index, true);
      return;
    }
    onclose();
    item.action?.();
  }

  function keydown(event: KeyboardEvent) {
    // A submenu opened by hovering leaves the focus here: any key but → closes it first.
    if (submenuIndex >= 0 && event.key !== 'ArrowRight') submenuIndex = -1;
    const keys: Record<string, () => void> = {
      ArrowDown: () => move(1),
      ArrowUp: () => move(-1),
      Home: () => {
        active = -1;
        move(1);
      },
      End: () => {
        active = 0;
        move(-1);
      },
      Enter: () => activate(active),
      ' ': () => activate(active),
      ArrowRight: () => {
        const item = items[active];
        if (!item || isSeparator(item) || !item.submenu || item.disabled) return;
        submenuIndex = -1;
        void tick().then(() => openSubmenu(active, true));
      },
      ArrowLeft: () => onback?.(),
      Escape: () => (onback ? onback() : onclose()),
      Tab: () => {},
    };
    const handler = keys[event.key];
    if (!handler) return;
    event.preventDefault();
    event.stopPropagation();
    handler();
  }

  function hover(index: number) {
    active = enabled(index) ? index : -1;
    clearTimeout(hoverTimer);
    const item = items[index];
    if (item && !isSeparator(item) && item.submenu && !item.disabled) {
      hoverTimer = setTimeout(() => void openSubmenu(index, false), 120);
    } else if (submenuIndex >= 0) {
      hoverTimer = setTimeout(() => (submenuIndex = -1), 120);
    }
  }

  function closeSubmenu() {
    submenuIndex = -1;
    menu?.focus();
  }
</script>

<div
  bind:this={menu}
  class="context-menu"
  role="menu"
  tabindex="-1"
  style:left="{left}px"
  style:top="{top}px"
  onkeydown={keydown}
  oncontextmenu={(e) => e.preventDefault()}
>
  {#each items as item, index (index)}
    {#if isSeparator(item)}
      <div class="separator" role="separator"></div>
    {:else}
      <div
        class="item"
        class:active={index === active}
        class:disabled={item.disabled}
        class:destructive={item.destructive}
        role={item.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
        aria-checked={item.checked}
        aria-disabled={item.disabled}
        aria-haspopup={item.submenu ? 'menu' : undefined}
        aria-expanded={item.submenu ? index === submenuIndex : undefined}
        tabindex="-1"
        data-index={index}
        onpointerenter={() => hover(index)}
        onclick={() => activate(index)}
        onkeydown={() => {}}
      >
        <span class="check"
          >{#if item.checked}<Check size={12} />{/if}</span
        >
        {#if item.icon}<item.icon size={14} />{/if}
        <span class="label">{item.label}</span>
        {#if item.shortcut}<span class="shortcut">{item.shortcut}</span>{/if}
        {#if item.submenu}<ChevronRight size={12} />{/if}
      </div>
    {/if}
  {/each}
</div>

{#if submenuIndex >= 0}
  {@const parent = items[submenuIndex]}
  {#if parent && !isSeparator(parent) && parent.submenu}
    {#key submenuIndex}
      <ContextMenu
        items={parent.submenu}
        x={submenuAt.x}
        y={submenuAt.y}
        parentWidth={submenuAt.width}
        {onclose}
        onback={closeSubmenu}
      />
    {/key}
  {/if}
{/if}

<style>
  .context-menu {
    position: fixed;
    z-index: 200;
    min-width: 180px;
    max-width: 320px;
    padding: 5px;
    border-radius: 10px;
    background: var(--glass-surface);
    box-shadow: var(--glass-edge), var(--shadow-popover);
    outline: none;
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 22px;
    padding: 0 8px 0 4px;
    border-radius: 6px;
    white-space: nowrap;
  }
  .item.active {
    background: var(--selection-accent);
    color: #fff;
  }
  .item.disabled {
    opacity: 0.4;
  }
  .item.destructive:not(.active) {
    color: var(--danger);
  }
  .check {
    display: inline-flex;
    width: 12px;
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .shortcut {
    margin-left: var(--space-4);
    color: var(--fg-tertiary);
  }
  .item.active .shortcut {
    color: rgba(255, 255, 255, 0.8);
  }
  .separator {
    height: 0.5px;
    margin: 5px 6px;
    background: var(--separator);
  }
</style>
