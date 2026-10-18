import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createAlertStore } from './alert.svelte';
import { createToastStore } from './toast.svelte';

describe('toast store', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('auto-hides after the duration', () => {
    const toasts = createToastStore({ duration: 2500 });
    toasts.show('Saved');
    expect(toasts.current?.message).toBe('Saved');
    vi.advanceTimersByTime(2499);
    expect(toasts.current).not.toBeNull();
    vi.advanceTimersByTime(1);
    expect(toasts.current).toBeNull();
  });

  it('queues messages', () => {
    const toasts = createToastStore({ duration: 1000 });
    toasts.show('one');
    toasts.show('two');
    expect(toasts.pending).toBe(1);
    vi.advanceTimersByTime(1000);
    expect(toasts.current?.message).toBe('two');
    toasts.dismiss();
    expect(toasts.current).toBeNull();
  });

  it('ignores empty and repeated messages', () => {
    const toasts = createToastStore();
    toasts.show('');
    expect(toasts.current).toBeNull();
    toasts.show('same');
    toasts.show('same');
    expect(toasts.pending).toBe(0);
    toasts.clear();
    expect(toasts.current).toBeNull();
  });
});

describe('alert store', () => {
  it('shows alerts one after another', () => {
    const alerts = createAlertStore();
    alerts.show('first');
    alerts.show('second');
    alerts.show('');
    expect(alerts.current).toBe('first');
    alerts.dismiss();
    expect(alerts.current).toBe('second');
    alerts.dismiss();
    expect(alerts.current).toBeNull();
  });
});
