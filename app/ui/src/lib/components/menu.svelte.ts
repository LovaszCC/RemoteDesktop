// Context menus: the item model and the one menu that can be open at a time (rendered by
// `ContextMenuHost`). `showContextMenu(event, items)` opens it at the pointer.

import type { Component } from 'svelte';

export type MenuItem =
  | {
      kind?: 'item';
      label: string;
      icon?: Component<{ size?: number | string }>;
      /** Shortcut text shown on the right (e.g. "⌘N"). */
      shortcut?: string;
      disabled?: boolean;
      destructive?: boolean;
      checked?: boolean;
      action?: () => void;
      /** Opens a submenu instead of acting. */
      submenu?: MenuItem[];
    }
  | { kind: 'separator' };

export interface OpenMenu {
  items: MenuItem[];
  x: number;
  y: number;
}

let open = $state<OpenMenu | null>(null);

export const contextMenu = {
  get open() {
    return open;
  },
};

/** Opens `items` at the pointer of `event` (or at `x`/`y`). */
export function showContextMenu(
  event: { clientX: number; clientY: number; preventDefault?: () => void },
  items: MenuItem[],
) {
  event.preventDefault?.();
  if (items.length === 0) return;
  open = { items, x: event.clientX, y: event.clientY };
}

export function closeContextMenu() {
  open = null;
}

export function isSeparator(item: MenuItem): item is { kind: 'separator' } {
  return item.kind === 'separator';
}
