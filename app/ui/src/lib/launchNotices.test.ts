import { describe, expect, it } from 'vitest';
import { installLaunchNotices } from './launchNotices';
import { createToastStore } from './stores/toast.svelte';
import { fakeBridge } from './testing/bridge';

describe('launch notices', () => {
  it('toasts the translated notice until stopped', async () => {
    const bridge = fakeBridge();
    const toast = createToastStore();
    const stop = installLaunchNotices(bridge.listen, toast);
    await Promise.resolve();
    const message = "Couldn't find the password field — password on the clipboard (30 s)";
    bridge.emit('launch-notice', { connectionName: 'dc', message });
    expect(toast.current?.message).toBe(message);
    stop();
    expect(bridge.listenerCount('launch-notice')).toBe(0);
  });
});
