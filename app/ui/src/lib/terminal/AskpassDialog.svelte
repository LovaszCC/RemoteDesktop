<script lang="ts">
  // The `ssh` question the vault couldn't answer: a password or key passphrase (secure field), an unknown
  // host key (Yes / No with ssh's text in monospace) or anything else (a text field). Cancel/Esc refuses.
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import SecureField from '../components/SecureField.svelte';
  import TextField from '../components/TextField.svelte';
  import { t } from '../i18n/index.svelte';
  import { askpass as defaultAskpass, type AskpassStore } from './askpass.svelte';
  import { askpassTitle } from './askpassModel';

  let { askpass = defaultAskpass }: { askpass?: AskpassStore } = $props();

  const prompt = $derived(askpass.current);
  let value = $state('');

  const refuse = () => void askpass.answer(null);
  function submit() {
    const answer = value;
    value = '';
    void askpass.answer(answer);
  }
  function confirm(yes: boolean) {
    void askpass.answer(yes ? 'yes' : 'no');
  }
</script>

{#if prompt}
  {#key prompt.requestId}
    <Modal title={askpassTitle(prompt.kind)} onclose={refuse} width={prompt.kind === 'confirm' ? 560 : 380}>
      {#if prompt.kind === 'confirm'}
        <pre class="prompt selectable" data-testid="askpass-text">{prompt.prompt}</pre>
      {:else}
        <p class="label">{prompt.prompt}</p>
        {#if prompt.kind === 'other'}
          <TextField bind:value aria-label={prompt.prompt} onsubmit={submit} autocomplete="off" />
        {:else}
          <SecureField
            bind:value
            aria-label={prompt.prompt}
            onsubmit={submit}
            autocomplete="off"
            revealable={false}
          />
        {/if}
      {/if}
      {#snippet footer()}
        {#if prompt.kind === 'confirm'}
          <Button onclick={() => confirm(false)}>{t('No')}</Button>
          <Button variant="primary" onclick={() => confirm(true)}>{t('Yes')}</Button>
        {:else}
          <Button onclick={refuse}>{t('Cancel')}</Button>
          <Button variant="primary" onclick={submit}>{t('OK')}</Button>
        {/if}
      {/snippet}
    </Modal>
  {/key}
{/if}

<style>
  .prompt {
    max-height: 260px;
    margin: 0;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: var(--font-size-callout);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .label {
    margin: 0 0 var(--space-2);
    overflow-wrap: anywhere;
  }
</style>
