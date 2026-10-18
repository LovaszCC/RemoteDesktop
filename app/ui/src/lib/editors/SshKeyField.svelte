<script lang="ts">
  import Button from '../components/Button.svelte';
  import TextField from '../components/TextField.svelte';
  import { viewDeps } from '../detail/deps';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';

  /**
   * Swift `SSHKeyField`: the key path with Browse… (starts in ~/.ssh, stores a `~`-relative path) and
   * Generate… (a new ed25519 pair in ~/.ssh named after `name`; nothing is uploaded).
   */
  let {
    value = $bindable(''),
    id,
    disabled = false,
    name = '',
  }: { value?: string; id?: string; disabled?: boolean; name?: string } = $props();
  const deps = viewDeps();
  let generating = $state(false);

  async function browse() {
    try {
      const path = await deps.api.pickSshKey();
      if (path) value = path;
    } catch (error) {
      deps.alerts.show(errorMessage(error));
    }
  }

  async function generate() {
    generating = true;
    try {
      value = await deps.api.sshKeyGenerate(name.trim());
      deps.toast.show(t('SSH key created: %@', value));
    } catch (error) {
      deps.alerts.show(errorMessage(error));
    } finally {
      generating = false;
    }
  }
</script>

<div class="ssh-key-field">
  <TextField {id} bind:value placeholder="~/.ssh/id_ed25519" aria-label={t('SSH key')} {disabled} />
  <Button {disabled} onclick={browse}>{t('Browse…')}</Button>
  <Button disabled={disabled || generating} onclick={generate}>{t('Generate…')}</Button>
</div>

<style>
  .ssh-key-field {
    display: flex;
    gap: var(--space-2);
  }
</style>
