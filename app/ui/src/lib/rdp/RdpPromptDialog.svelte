<script lang="ts">
  // The RDP session's questions (Swift `CertificatePrompt` and `CredentialPrompt`):
  // - the server certificate: server, name, subject, issuer and SHA-256 fingerprint, with warnings for a
  //   changed certificate and a host name mismatch; Accept (permanently) / Just this time / Reject (Esc);
  // - the NLA sign-in: user name, domain and password, "Save to this connection" when the connection has no
  //   linked credential; Connect / Cancel (Esc). The password goes to Rust once and is then cleared.
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from '../components/Button.svelte';
  import Checkbox from '../components/Checkbox.svelte';
  import Modal from '../components/Modal.svelte';
  import SecureField from '../components/SecureField.svelte';
  import TextField from '../components/TextField.svelte';
  import { t } from '../i18n/index.svelte';
  import { rdpPrompts as defaultPrompts, type RdpPromptsStore } from './prompts.svelte';

  let { prompts = defaultPrompts }: { prompts?: RdpPromptsStore } = $props();

  const prompt = $derived(prompts.current);

  let username = $state('');
  let domain = $state('');
  let password = $state('');
  let save = $state(false);

  // A new sign-in prompt starts from its pre-filled values.
  $effect.pre(() => {
    if (prompt?.kind !== 'credentials') return;
    username = prompt.username;
    domain = prompt.domain;
    password = '';
    save = false;
  });

  function signIn() {
    if (prompt?.kind !== 'credentials') return;
    const answer = {
      username: username.trim(),
      domain: domain.trim(),
      password,
      save: prompt.canSave && save,
    };
    password = '';
    void prompts.answerCredentials(answer);
  }

  function cancelSignIn() {
    password = '';
    void prompts.answerCredentials(null);
  }
</script>

{#if prompt?.kind === 'certificate'}
  {#key prompt.requestId}
    <Modal
      title={prompt.changed
        ? t('The certificate of %@ has changed!', prompt.connectionName)
        : t('Unknown certificate: %@', prompt.connectionName)}
      onclose={() => void prompts.answerCertificate('reject')}
      width={560}
    >
      <div class="details selectable">
        <p>{t('Server: %@:%u', prompt.host, prompt.port)}</p>
        <p>{t('Name: %@', prompt.commonName)}</p>
        <p>{t('Subject: %@', prompt.subject)}</p>
        <p>{t('Issuer: %@', prompt.issuer)}</p>
        <p class="mono" data-testid="rdp-fingerprint">{t('Fingerprint: %@', prompt.fingerprint)}</p>
      </div>
      {#if prompt.nameMismatch}
        <p class="warning" role="alert">
          <TriangleAlert size={14} />{t('Warning: the certificate is not for this host name.')}
        </p>
      {/if}
      {#if prompt.changed}
        <p class="warning" role="alert">
          <TriangleAlert size={14} />{t(
            'Warning: you accepted a different certificate for this server before.',
          )}
        </p>
      {/if}
      {#snippet footer()}
        <Button onclick={() => void prompts.answerCertificate('reject')}>{t('Reject')}</Button>
        <Button onclick={() => void prompts.answerCertificate('acceptOnce')}>{t('Just this time')}</Button>
        <Button
          variant={prompt.changed || prompt.nameMismatch ? 'destructive' : 'primary'}
          onclick={() => void prompts.answerCertificate('acceptPermanently')}>{t('Accept')}</Button
        >
      {/snippet}
    </Modal>
  {/key}
{:else if prompt?.kind === 'credentials'}
  {#key prompt.requestId}
    <Modal title={t('Sign in to %@', prompt.connectionName)} onclose={cancelSignIn} width={400}>
      <p class="message">{t('The server requires Network Level Authentication.')}</p>
      {#if prompt.rejected}
        <p class="warning" role="alert">
          <TriangleAlert size={14} />{t('The server rejected the user name or password.')}
        </p>
      {/if}
      <div class="grid">
        <label for="rdp-username">{t('Username')}</label>
        <TextField id="rdp-username" bind:value={username} onsubmit={signIn} autofocus={username === ''} />
        <label for="rdp-domain">{t('Domain')}</label>
        <TextField id="rdp-domain" bind:value={domain} onsubmit={signIn} />
        <label for="rdp-password">{t('Password')}</label>
        <SecureField
          id="rdp-password"
          bind:value={password}
          onsubmit={signIn}
          revealable={false}
          autofocus={username !== ''}
        />
        {#if prompt.canSave}
          <span></span>
          <Checkbox bind:checked={save} label={t('Save to this connection')} />
        {/if}
      </div>
      {#snippet footer()}
        <Button onclick={cancelSignIn}>{t('Cancel')}</Button>
        <Button variant="primary" onclick={signIn}>{t('Connect')}</Button>
      {/snippet}
    </Modal>
  {/key}
{/if}

<style>
  .details p {
    margin: 0 0 4px;
    overflow-wrap: anywhere;
  }
  .mono {
    font-family: var(--font-mono);
    font-size: var(--font-size-callout);
  }
  .message {
    margin: 0 0 var(--space-3);
    overflow-wrap: anywhere;
  }
  .warning {
    display: flex;
    align-items: flex-start;
    gap: var(--space-1);
    margin: var(--space-3) 0 0;
    color: var(--warning);
    font-weight: 600;
  }
  .warning :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .grid {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    align-items: center;
    gap: 8px var(--space-3);
    margin-top: var(--space-3);
  }
  .grid label {
    justify-self: end;
  }
</style>
