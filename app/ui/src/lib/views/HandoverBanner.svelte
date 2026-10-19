<script lang="ts">
  // The welcome banner of the first launch after the Swift app (its settings were imported). On macOS it
  // also asks for the Accessibility permission again while it is missing: the new app's signature differs,
  // so the old grant doesn't apply.
  import { api as defaultApi, type Api } from '../api';
  import Banner from '../components/Banner.svelte';
  import Button from '../components/Button.svelte';
  import { t } from '../i18n/index.svelte';
  import { app as defaultApp, type AppStore } from '../stores/app.svelte';

  interface Props {
    api?: Pick<Api, 'accessibilityStatus' | 'openAccessibilitySettings'>;
    app?: Pick<AppStore, 'handoverNotice' | 'platform' | 'dismissHandover'>;
  }

  let { api = defaultApi, app = defaultApp }: Props = $props();

  let accessibility = $state<boolean | null>(null);
  $effect(() => {
    if (app.handoverNotice && app.platform === 'macos') {
      api
        .accessibilityStatus()
        .then((trusted) => (accessibility = trusted))
        .catch(() => {});
    }
  });
  const askAccessibility = $derived(app.platform === 'macos' && accessibility === false);
</script>

{#if app.handoverNotice}
  <Banner
    kind="info"
    message={askAccessibility
      ? t(
          'Welcome to the new RemoteDesktop. Your connections and settings were carried over. Grant the Accessibility permission again for RDP password autofill.',
        )
      : t('Welcome to the new RemoteDesktop. Your connections and settings were carried over.')}
  >
    {#snippet actions()}
      {#if askAccessibility}
        <Button onclick={() => void api.openAccessibilitySettings().catch(() => {})}>
          {t('Open System Settings')}
        </Button>
      {/if}
      <Button onclick={() => void app.dismissHandover().catch(() => {})}>{t('Dismiss')}</Button>
    {/snippet}
  </Banner>
{/if}
