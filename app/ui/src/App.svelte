<script lang="ts">
  import { onMount } from 'svelte';
  import AlertHost from './lib/components/AlertHost.svelte';
  import ContextMenuHost from './lib/components/ContextMenuHost.svelte';
  import ToastHost from './lib/components/ToastHost.svelte';
  import { installCommands } from './lib/commands.svelte';
  import { installEntryRequests } from './lib/entryRequests';
  import { installLaunchNotices } from './lib/launchNotices';
  import SettingsView from './lib/settings/SettingsView.svelte';
  import { settingsWindow } from './lib/stores/settingsWindow.svelte';
  import { errorMessage } from './lib/errors';
  import { alerts } from './lib/stores/alert.svelte';
  import { app } from './lib/stores/app.svelte';
  import { library } from './lib/stores/library.svelte';
  import { settings } from './lib/stores/settings.svelte';
  import RootView from './lib/views/RootView.svelte';
  import { forwards } from './lib/forwards/forwards.svelte';
  import HostKeyDialog from './lib/forwards/HostKeyDialog.svelte';
  import { hostKeys } from './lib/forwards/hostKeys.svelte';
  import AskpassDialog from './lib/terminal/AskpassDialog.svelte';
  import { askpass } from './lib/terminal/askpass.svelte';
  import { sessions } from './lib/terminal/sessions.svelte';
  import { rdpSessions } from './lib/rdp/sessions.svelte';
  import RdpPromptDialog from './lib/rdp/RdpPromptDialog.svelte';
  import { rdpPrompts } from './lib/rdp/prompts.svelte';
  import { terminals } from './lib/stores/terminals.svelte';
  import UpdateDialog from './lib/updates/UpdateDialog.svelte';
  import { updates } from './lib/updates/updates.svelte';
  import { vpn } from './lib/vpn/vpn.svelte';
  import VpnProgressDialog from './lib/vpn/VpnProgressDialog.svelte';

  onMount(() => {
    const commands = installCommands();
    const stopLaunchNotices = installLaunchNotices();
    const entryRequests = installEntryRequests();
    // Settings first so the first screen already has the right language.
    void (async () => {
      const report = (error: unknown) => alerts.show(errorMessage(error));
      await settings.start().catch(report);
      await Promise.all([
        sessions.start().catch(report),
        rdpSessions.start().catch(report),
        rdpPrompts.start().catch(report),
        askpass.start().catch(report),
        forwards.start().catch(report),
        hostKeys.start().catch(report),
      ]);
      void terminals.load();
      await Promise.all([app.start().catch(report), library.start().catch(report)]);
      // Connect with VPN is macOS only (MacVPN).
      if (app.platform === 'macos') void vpn.start().catch(report);
      // A launcher that started the app: its entry opens once the library is there.
      void entryRequests.check();
      // The silent launch check (Swift `checkAutomaticallyIfDue`): Rust skips it when off or not due.
      void updates.checkAutomatically();
    })();
    return () => {
      commands.stop();
      stopLaunchNotices();
      entryRequests.stop();
      app.stop();
      library.stop();
      settings.stop();
      sessions.stop();
      rdpSessions.stop();
      rdpPrompts.stop();
      askpass.stop();
      forwards.stop();
      hostKeys.stop();
      vpn.stop();
    };
  });
</script>

<RootView />
{#if settingsWindow.isOpen && app.phase?.kind !== 'lockConflict' && app.phase?.kind !== 'needsUpdate'}
  <SettingsView />
{/if}
<ToastHost />
<ContextMenuHost />
<AskpassDialog />
<RdpPromptDialog />
<HostKeyDialog />
<HostKeyDialog store={sessions.hostKeyPrompts} />
<UpdateDialog />
<VpnProgressDialog />
<AlertHost />
