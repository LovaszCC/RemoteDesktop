// Late launch notices (`launch-notice`): the macOS RDP autofill found no password field and put the
// password on the clipboard instead (Swift `RDPLauncher` `onFallback`). Shown as a toast.

import { onLaunchNotice, tauriBridge, type Listen, type Unlisten } from './api';
import { t } from './i18n/index.svelte';
import { toast as defaultToast, type ToastStore } from './stores/toast.svelte';

/** Starts listening; returns the stop function. */
export function installLaunchNotices(listen: Listen = tauriBridge.listen, toast: ToastStore = defaultToast) {
  let unlisten: Unlisten | undefined;
  let stopped = false;
  void onLaunchNotice(listen, (notice) => toast.show(t(notice.message)))
    .then((off) => {
      if (stopped) off();
      else unlisten = off;
    })
    .catch(() => {});
  return () => {
    stopped = true;
    unlisten?.();
  };
}
