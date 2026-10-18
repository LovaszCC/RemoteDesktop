// Canvas input → `rdp_input` events. Coordinates are canvas CSS pixels (Rust maps them onto the desktop,
// which may lag the view while a resize is pending). Keys go by `KeyboardEvent.code` (the physical key)
// plus `.key` (what it types); Rust translates them.

import type { RdpInput } from '../types';

/** The canvas box the coordinates are relative to. */
export interface Box {
  left: number;
  top: number;
}

/** A point of a mouse event inside the canvas, in CSS pixels. */
export function point(event: Pick<MouseEvent, 'clientX' | 'clientY'>, box: Box): { x: number; y: number } {
  return { x: Math.max(0, event.clientX - box.left), y: Math.max(0, event.clientY - box.top) };
}

export function mouseMove(event: Pick<MouseEvent, 'clientX' | 'clientY'>, box: Box): RdpInput {
  return { type: 'mouseMove', ...point(event, box) };
}

export function mouseButton(
  event: Pick<MouseEvent, 'clientX' | 'clientY' | 'button'>,
  down: boolean,
  box: Box,
): RdpInput {
  return { type: 'mouseButton', button: event.button, down, ...point(event, box) };
}

export function wheel(
  event: Pick<WheelEvent, 'clientX' | 'clientY' | 'deltaX' | 'deltaY' | 'deltaMode'>,
  box: Box,
): RdpInput {
  return {
    type: 'wheel',
    deltaX: event.deltaX,
    deltaY: event.deltaY,
    deltaMode: event.deltaMode,
    ...point(event, box),
  };
}

export function key(event: Pick<KeyboardEvent, 'code' | 'key'>, down: boolean): RdpInput {
  return { type: 'key', code: event.code, key: event.key, down };
}

/**
 * The keys pressed on the canvas since the last release-all. A release whose press the session never
 * saw (it went to an app shortcut, or happened before the focus came) is dropped: on macOS a lone ⌘
 * release would otherwise type the Windows key.
 */
export class KeyTracker {
  // Not rendered.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #down = new Set<string>();

  /** Records a press; always sent. */
  press(code: string) {
    this.#down.add(code);
  }

  /** Whether the release of `code` goes to the session. */
  release(code: string): boolean {
    return this.#down.delete(code);
  }

  /** Forgets every press (after `rdp_release_all_keys`). */
  clear() {
    this.#down.clear();
  }

  get size(): number {
    return this.#down.size;
  }
}
