import { describe, expect, it } from 'vitest';
import { fakeBridge } from '../testing/bridge';
import { browserName, browserPickerOptions, createBrowsersStore } from './browsers.svelte';

const installed = [
  { id: 'com.brave.Browser', name: 'Brave Browser' },
  { id: 'org.mozilla.firefox', name: 'Firefox' },
];

describe('browsers', () => {
  it('names installed browsers and falls back to the id', () => {
    expect(browserName('org.mozilla.firefox', installed)).toBe('Firefox');
    expect(browserName('com.example.gone', installed)).toBe('com.example.gone');
  });

  it('builds the picker with the default, the installed list and a missing saved one', () => {
    expect(browserPickerOptions(null, installed)).toEqual([
      { value: '', label: 'Default' },
      { value: 'com.brave.Browser', label: 'Brave Browser' },
      { value: 'org.mozilla.firefox', label: 'Firefox' },
    ]);
    expect(browserPickerOptions('com.example.gone', installed).at(-1)).toEqual({
      value: 'com.example.gone',
      label: 'com.example.gone (not installed)',
    });
    expect(browserPickerOptions('com.brave.Browser', installed)).toHaveLength(3);
  });

  it('loads once and retries after a failure', async () => {
    let fail = true;
    const bridge = fakeBridge({
      installed_browsers: () => {
        if (fail) throw new Error('nope');
        return installed;
      },
    });
    const store = createBrowsersStore(bridge);
    await store.load();
    expect(store.installed).toEqual([]);
    fail = false;
    await store.load();
    await store.load();
    expect(store.installed).toEqual(installed);
    expect(store.name('com.brave.Browser')).toBe('Brave Browser');
    expect(bridge.invoke).toHaveBeenCalledTimes(2);
  });
});
