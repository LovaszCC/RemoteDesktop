<script lang="ts">
  import SecureField from '../components/SecureField.svelte';
  import { t } from '../i18n/index.svelte';
  import { clearSecret, keepSecret, keepsStored, secretPlaceholder, type SecretState } from './model';

  /**
   * A password or passphrase. It starts empty: "unchanged" means the stored secret is kept, "Clear"
   * removes it on save, and typing replaces it.
   */
  interface Props {
    value: SecretState;
    id?: string;
    label: string;
    disabled?: boolean;
  }

  let { value = $bindable(), id, label, disabled = false }: Props = $props();
</script>

<div class="secret-input">
  <SecureField
    {id}
    bind:value={value.text}
    placeholder={secretPlaceholder(value)}
    aria-label={label}
    {disabled}
  />
  {#if keepsStored(value)}
    <button type="button" class="link" {disabled} onclick={() => (value = clearSecret(value))}>
      {t('Clear')}
    </button>
  {:else if value.stored && value.cleared && value.text === ''}
    <span class="note">{t('Removed on save')}</span>
    <button type="button" class="link" {disabled} onclick={() => (value = keepSecret(value))}>
      {t('Undo')}
    </button>
  {/if}
</div>

<style>
  .secret-input {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .link {
    flex: none;
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font-size: var(--font-size-callout);
  }
  .link:hover:not(:disabled) {
    text-decoration: underline;
  }
  .note {
    flex: none;
    color: var(--fg-secondary);
    font-size: var(--font-size-callout);
  }
</style>
