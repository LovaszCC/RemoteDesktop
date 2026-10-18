// A fake Tauri bridge for store tests: scripted command replies and manually emitted events.

import { vi } from 'vitest';
import type { Invoke, Listen } from '../api';

export function fakeBridge(
  replies: Record<string, unknown | ((args?: Record<string, unknown>) => unknown)> = {},
) {
  const handlers = new Map<string, Set<(payload: unknown) => void>>();
  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (!(command in replies)) throw new Error(`unexpected command ${command}`);
    const reply = replies[command];
    return typeof reply === 'function' ? (reply as (a?: Record<string, unknown>) => unknown)(args) : reply;
  });
  const listen = vi.fn(async (event: string, handler: (payload: unknown) => void) => {
    const set = handlers.get(event) ?? new Set();
    set.add(handler);
    handlers.set(event, set);
    return () => set.delete(handler);
  });
  return {
    invoke: invoke as unknown as Invoke & typeof invoke,
    listen: listen as unknown as Listen & typeof listen,
    replies,
    emit(event: string, payload: unknown) {
      for (const handler of handlers.get(event) ?? []) handler(payload);
    },
    listenerCount(event: string) {
      return handlers.get(event)?.size ?? 0;
    },
  };
}
