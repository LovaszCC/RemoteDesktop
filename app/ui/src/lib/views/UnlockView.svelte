<script lang="ts">
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Lock from '@lucide/svelte/icons/lock';
  import { onMount } from 'svelte';
  import { toCommandError } from '../api';
  import Button from '../components/Button.svelte';
  import Checkbox from '../components/Checkbox.svelte';
  import SecureField from '../components/SecureField.svelte';
  import { errorMessage } from '../errors';
  import { middleTruncate } from '../format';
  import { t } from '../i18n/index.svelte';
  import { alerts } from '../stores/alert.svelte';
  import { app } from '../stores/app.svelte';

  interface Props {
    needsSetup: boolean;
    /** "Other database…". */
    onotherdatabase: () => void;
  }

  let { needsSetup, onotherdatabase }: Props = $props();

  let password = $state('');
  let confirmation = $state('');
  let remember = $state(app.status?.rememberMasterKey ?? true);
  let wrongPassword = $state(false);
  let busy = $state(false);
  let field: HTMLInputElement | undefined = $state();

  const canSubmit = $derived(password !== '' && (!needsSetup || password === confirmation) && !busy);
  const rememberLabel = $derived(
    app.platform === 'macos' ? t('Remember on this Mac (Keychain)') : t('Remember on this computer'),
  );

  onMount(() => field?.focus());

  async function submit() {
    if (!canSubmit) return;
    busy = true;
    try {
      if (needsSetup) await app.createVault(password, remember);
      else await app.unlock(password, remember);
    } catch (error) {
      if (toCommandError(error).code === 'wrongPassword') {
        wrongPassword = true;
        password = '';
        field?.focus();
      } else {
        alerts.show(errorMessage(error));
      }
    } finally {
      busy = false;
    }
  }
</script>

<div class="unlock">
  <div class="icon">
    {#if needsSetup}<KeyRound size={44} strokeWidth={1.25} />{:else}<Lock size={44} strokeWidth={1.25} />{/if}
  </div>

  <div class="heading">
    <h1>{needsSetup ? t('Create master password') : t('RemoteDesktop is locked')}</h1>
    <p class="subtitle" title={needsSetup ? undefined : (app.databasePath ?? undefined)}>
      {#if needsSetup}
        {t("It encrypts your saved passwords. If you forget it, it can't be recovered.")}
      {:else}
        {middleTruncate(app.databasePath ?? '')}
      {/if}
    </p>
  </div>

  <div class="fields">
    <SecureField
      bind:value={password}
      bind:element={field}
      placeholder={t('Master password')}
      aria-label={t('Master password')}
      invalid={wrongPassword}
      oninput={() => (wrongPassword = false)}
      onsubmit={submit}
    />
    {#if needsSetup}
      <SecureField
        bind:value={confirmation}
        placeholder={t('Master password again')}
        aria-label={t('Master password again')}
        onsubmit={submit}
      />
    {/if}
    {#if wrongPassword}
      <p class="error" role="alert">{t('Wrong password')}</p>
    {/if}
  </div>

  <Checkbox bind:checked={remember} label={rememberLabel} />

  <Button variant="primary" disabled={!canSubmit} onclick={submit}>
    {needsSetup ? t('Create') : t('Unlock')}
  </Button>

  <Button variant="link" class="other" onclick={onotherdatabase}>{t('Other database…')}</Button>
</div>

<style>
  .unlock {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 18px;
    height: 100%;
    padding: 40px;
  }
  .icon {
    color: var(--fg-secondary);
  }
  .heading {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    max-width: 440px;
    text-align: center;
  }
  h1 {
    margin: 0;
    font-size: var(--font-size-large-title);
    font-weight: 600;
  }
  .subtitle {
    margin: 0;
    color: var(--fg-secondary);
    font-size: var(--font-size-callout);
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 280px;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: var(--font-size-small);
  }
  .unlock :global(.other) {
    font-size: var(--font-size-callout);
  }
</style>
