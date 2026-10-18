<script lang="ts">
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import { untrack } from 'svelte';
  import Button from '../components/Button.svelte';
  import Modal from '../components/Modal.svelte';
  import TextField from '../components/TextField.svelte';
  import { viewDeps } from '../detail/deps';
  import { errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import type { CredentialEditorRequest } from '../stores/editor.svelte';
  import FormRow from './FormRow.svelte';
  import FormSection from './FormSection.svelte';
  import { buildCredentialSave, canSave, credentialFormFrom, newCredentialForm } from './model';
  import SecretInput from './SecretInput.svelte';
  import SshKeyField from './SshKeyField.svelte';

  /** Swift `CredentialEditorView`: a save error (wrong key passphrase, unreadable key) keeps it open. */
  let { request, onclose }: { request: CredentialEditorRequest; onclose: () => void } = $props();
  const deps = viewDeps();
  const uid = Math.random().toString(36).slice(2);
  const id = (name: string) => `cred-${name}-${uid}`;

  const editedId = untrack(() => (request.mode === 'edit' ? request.id : null));
  const isNew = editedId === null;
  let form = $state(
    untrack(() => {
      const existing = editedId ? deps.library.credential(editedId) : undefined;
      return existing ? credentialFormFrom(existing) : newCredentialForm();
    }),
  );
  let failure = $state<string | null>(null);
  let saving = $state(false);

  $effect(() => {
    if (editedId && !deps.library.credential(editedId)) onclose();
  });

  function submitOnEnter(event: KeyboardEvent) {
    if (event.key === 'Enter' && event.target instanceof HTMLInputElement && !event.isComposing) {
      event.preventDefault();
      void save();
    }
  }

  async function save() {
    if (!canSave(form.name) || saving || deps.readOnly) return;
    saving = true;
    failure = null;
    const { credential, password, passphrase } = buildCredentialSave(form);
    try {
      const savedId = await deps.api.saveCredential(editedId, credential, password, passphrase);
      onclose();
      if (isNew) deps.library.select(`k:${savedId}`);
    } catch (error) {
      failure = errorMessage(error);
    } finally {
      saving = false;
    }
  }
</script>

<Modal title={isNew ? t('New credential') : t('Edit credential')} {onclose} width={480}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <form
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
    onkeydown={submitOnEnter}
  >
    <FormSection>
      <FormRow label={t('Name')} for={id('name')}>
        <TextField id={id('name')} bind:value={form.name} placeholder={t('e.g. Domain Admin')} autofocus />
      </FormRow>
    </FormSection>
    <FormSection title={t('Windows / RDP')}>
      <FormRow label={t('Username')} for={id('user')}>
        <TextField id={id('user')} bind:value={form.username} />
      </FormRow>
      <FormRow label={t('Domain')} for={id('domain')}>
        <TextField id={id('domain')} bind:value={form.domain} placeholder={t('optional, not used for SSH')} />
      </FormRow>
      <FormRow label={t('Password')} for={id('password')}>
        <SecretInput id={id('password')} label={t('Password')} bind:value={form.password} />
      </FormRow>
    </FormSection>
    <FormSection title={t('SSH key (optional)')}>
      <FormRow label={t('SSH key')} for={id('key')}>
        <SshKeyField id={id('key')} bind:value={form.sshKeyPath} name={form.name} />
      </FormRow>
      {#if form.sshKeyPath.trim() !== ''}
        <FormRow label={t('Key passphrase')} for={id('passphrase')}>
          <SecretInput id={id('passphrase')} label={t('Key passphrase')} bind:value={form.passphrase} />
        </FormRow>
      {/if}
    </FormSection>
    <FormSection title={t('Notes')}>
      <textarea bind:value={form.notes} aria-label={t('Notes')} rows="3"></textarea>
    </FormSection>
  </form>
  {#snippet footer()}
    {#if failure}
      <p class="failure" role="alert"><TriangleAlert size={13} />{failure}</p>
    {/if}
    <Button onclick={onclose}>{t('Cancel')}</Button>
    <Button variant="primary" disabled={!canSave(form.name) || saving || deps.readOnly} onclick={save}>
      {isNew ? t('Create') : t('Save')}
    </Button>
  {/snippet}
</Modal>

<style>
  textarea {
    width: 100%;
    margin: 6px 0;
    padding: 4px 7px;
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius-small);
    background: var(--bg-control);
    resize: vertical;
    outline: none;
  }
  textarea:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus-ring);
  }
  .failure {
    display: flex;
    flex: 1;
    align-items: flex-start;
    gap: 4px;
    margin: 0;
    color: var(--danger);
    overflow-wrap: anywhere;
  }
</style>
