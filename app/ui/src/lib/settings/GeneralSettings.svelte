<script lang="ts">
  // Swift `SettingsView.general`: language, SSH, WinBox, updates, database, security, master password, RDP.
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import CircleX from '@lucide/svelte/icons/circle-x';
  import FilePlus from '@lucide/svelte/icons/file-plus';
  import FolderOpen from '@lucide/svelte/icons/folder-open';
  import { api as defaultApi, type Api } from '../api';
  import Button from '../components/Button.svelte';
  import SecureField from '../components/SecureField.svelte';
  import Select from '../components/Select.svelte';
  import Toggle from '../components/Toggle.svelte';
  import FormRow from '../editors/FormRow.svelte';
  import FormSection from '../editors/FormSection.svelte';
  import AccentPicker from './AccentPicker.svelte';
  import { errorMessage } from '../errors';
  import { i18n, t } from '../i18n/index.svelte';
  import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
  import { app as defaultApp, type AppStore } from '../stores/app.svelte';
  import { appearance as defaultAppearance, THEMES, type AppearanceStore } from '../stores/appearance.svelte';
  import { settings as defaultSettings, type SettingsStore } from '../stores/settings.svelte';
  import {
    externalTerminalOptions,
    terminals as defaultTerminals,
    type TerminalsStore,
  } from '../stores/terminals.svelte';
  import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
  import type { Language, PickMode, RdpEngineInfoDto, SelectionShows, WinboxStatusDto } from '../types';
  import { updateStatusText } from '../updates/model';
  import { updates as defaultUpdates, type UpdateStore } from '../updates/updates.svelte';
  import {
    LANGUAGE_OPTIONS,
    canChangePassword,
    lastCheckedText,
    selectionShowsOptions,
    revealLabel,
    rememberKeyLabel,
    winboxDetectedText,
    winboxPathText,
  } from './model';

  interface Props {
    api?: Api;
    app?: AppStore;
    settings?: SettingsStore;
    toast?: ToastStore;
    alerts?: AlertStore;
    terminals?: TerminalsStore;
    updates?: UpdateStore;
    appearance?: Pick<AppearanceStore, 'accent' | 'setAccent' | 'theme' | 'setTheme'>;
  }

  let {
    api = defaultApi,
    app = defaultApp,
    settings = defaultSettings,
    toast = defaultToast,
    alerts = defaultAlerts,
    terminals = defaultTerminals,
    updates = defaultUpdates,
    appearance = defaultAppearance,
  }: Props = $props();

  $effect(() => {
    void terminals.load();
  });

  let winbox = $state<WinboxStatusDto | null>(null);
  const refreshWinbox = () =>
    api
      .winboxStatus()
      .then((status) => (winbox = status))
      .catch(() => {});
  $effect(() => {
    void refreshWinbox();
  });

  let engine = $state<RdpEngineInfoDto | null>(null);
  $effect(() => {
    api
      .rdpEngineInfo()
      .then((info) => (engine = info))
      .catch(() => {});
  });

  let current = $state('');
  let next = $state('');
  let again = $state('');
  let busy = $state(false);

  const values = $derived(settings.settings);
  const platform = $derived(app.platform);
  const canChange = $derived(canChangePassword({ current, next, again }) && !busy);
  const lastChecked = $derived(lastCheckedText(values?.lastUpdateCheck ?? null, i18n.locale));
  const updateStatus = $derived(updateStatusText(updates.phase));
  const checking = $derived(updates.phase.kind === 'checking' || updates.phase.kind === 'installing');

  async function perform(work: () => Promise<unknown>) {
    try {
      await work();
    } catch (error) {
      alerts.show(errorMessage(error));
    }
  }

  const setLanguage = (language: Language) => perform(() => settings.setLanguage(language));
  const setSelectionShows = (selectionShows: SelectionShows) =>
    perform(() => settings.update({ selectionShows }));
  const setTerminal = (externalTerminal: string) => perform(() => settings.update({ externalTerminal }));
  const setChecks = (checkForUpdatesAutomatically: boolean) =>
    perform(() => settings.update({ checkForUpdatesAutomatically }));
  const setRemember = (rememberMasterKey: boolean) => perform(() => settings.update({ rememberMasterKey }));
  const choose = (mode: PickMode) => perform(() => app.chooseDatabase(mode));

  let accessibility = $state<boolean | null>(null);
  const checkAccessibility = () =>
    api
      .accessibilityStatus()
      .then((trusted) => (accessibility = trusted))
      .catch(() => {});
  $effect(() => {
    if (platform === 'macos') void checkAccessibility();
  });
  const openAccessibility = () => perform(() => api.openAccessibilitySettings());

  async function setWinbox(winboxPath: string) {
    await perform(() => settings.update({ winboxPath }));
    await refreshWinbox();
  }

  async function chooseWinbox() {
    let picked: string | null = null;
    await perform(async () => {
      picked = await api.pickWinbox();
    });
    if (picked) await setWinbox(picked);
  }

  async function changePassword() {
    if (!canChange) return;
    busy = true;
    try {
      await app.changePassword(current, next);
      current = '';
      next = '';
      again = '';
      toast.show(t('Master password changed'));
    } catch (error) {
      alerts.show(errorMessage(error));
    } finally {
      busy = false;
    }
  }
</script>

<div class="general">
  <FormSection title={t('Appearance')}>
    <FormRow label={t('Theme')}>
      <Select
        value={appearance.theme}
        options={THEMES.map((theme) => ({ value: theme.value, label: t(theme.label) }))}
        label={t('Theme')}
        onchange={(value) => appearance.setTheme(value)}
      />
    </FormRow>
    <FormRow label={t('Accent color')}>
      <AccentPicker {appearance} />
    </FormRow>
  </FormSection>

  <FormSection title={t('Language')}>
    <FormRow label={t('Language')}>
      <Select
        value={values?.language ?? 'en'}
        options={LANGUAGE_OPTIONS}
        label={t('Language')}
        onchange={(v) => void setLanguage(v)}
      />
    </FormRow>
  </FormSection>

  <FormSection title={t('Sidebar')}>
    <FormRow
      label={t('Clicking an entry with an open connection shows')}
      hint={t('With more than one open tab, or only in a split view, Details is shown.')}
    >
      <Select
        value={values?.selectionShows ?? 'details'}
        options={selectionShowsOptions()}
        label={t('Clicking an entry with an open connection shows')}
        onchange={(v) => void setSelectionShows(v)}
      />
    </FormRow>
  </FormSection>

  <FormSection title={t('SSH')}>
    <FormRow
      label={t('External terminal')}
      hint={t('Connections set to External terminal open in a new window of this app.')}
    >
      <Select
        value={values?.externalTerminal ?? ''}
        options={externalTerminalOptions(values?.externalTerminal ?? null, terminals.installed)}
        label={t('External terminal')}
        onchange={(v) => void setTerminal(v)}
      />
    </FormRow>
  </FormSection>

  <FormSection title="WinBox">
    <FormRow label={t('Detected')}>
      <span class="path selectable value">{winboxDetectedText(winbox?.detectedPath)}</span>
    </FormRow>
    <FormRow label={t('WinBox path')}>
      <span class="path selectable">{winboxPathText(values?.winboxPath ?? null)}</span>
    </FormRow>
    <div class="row inline">
      <Button onclick={() => void chooseWinbox()}><FolderOpen size={14} />{t('Choose…')}</Button>
      <Button disabled={!values?.winboxPath} onclick={() => void setWinbox('')}>{t('Use detected')}</Button>
      {#if winbox?.configuredExists === false}
        <span class="caption warning">{t("The file doesn't exist.")}</span>
      {/if}
    </div>
    {#if platform === 'linux'}
      <p class="caption row">{t('A winbox64.exe runs through Wine.')}</p>
    {/if}
    <p class="caption row">
      {t(
        'WinBox only accepts the password on its command line: other users of this computer can see it in the process list while WinBox runs.',
      )}
    </p>
  </FormSection>

  <FormSection title={t('Updates')}>
    <div class="row">
      <Toggle
        label={t('Check for updates automatically')}
        checked={values?.checkForUpdatesAutomatically ?? true}
        onchange={(v) => void setChecks(v)}
      />
    </div>
    <div class="row inline">
      <Button disabled={checking} onclick={() => void updates.checkManually()}>{t('Check now')}</Button>
      {#if updateStatus}<span class="caption" data-testid="update-status">{updateStatus}</span>{/if}
      <span class="spacer"></span>
      {#if lastChecked}<span class="caption">{lastChecked}</span>{/if}
    </div>
  </FormSection>

  <FormSection title={t('Database')}>
    <FormRow label={t('File')}>
      <span class="path selectable">{app.databasePath ?? values?.databasePath ?? '—'}</span>
    </FormRow>
    <div class="row inline">
      <Button onclick={() => void choose('open')}><FolderOpen size={14} />{t('Open existing…')}</Button>
      <Button onclick={() => void choose('create')}><FilePlus size={14} />{t('Create new…')}</Button>
      <Button disabled={!app.databasePath} onclick={() => void perform(() => app.revealDatabase())}>
        {revealLabel(platform)}
      </Button>
    </div>
    <p class="caption row">
      {t('Can live in iCloud Drive or Dropbox — passwords are encrypted with the master password.')}
    </p>
  </FormSection>

  <FormSection title={t('Security')}>
    <div class="row">
      <Toggle
        label={rememberKeyLabel(platform)}
        checked={values?.rememberMasterKey ?? true}
        onchange={(v) => void setRemember(v)}
      />
    </div>
    <div class="row inline">
      <Button disabled={!app.isUnlocked} onclick={() => void perform(() => app.lock())}
        >{t('Lock now')}</Button
      >
    </div>
  </FormSection>

  {#if app.isUnlocked}
    <FormSection title={t('Change master password')}>
      <FormRow label={t('Current')}>
        <SecureField bind:value={current} aria-label={t('Current')} autocomplete="current-password" />
      </FormRow>
      <FormRow label={t('New')}>
        <SecureField bind:value={next} aria-label={t('New')} autocomplete="new-password" />
      </FormRow>
      <FormRow label={t('New again')}>
        <SecureField
          bind:value={again}
          aria-label={t('New again')}
          autocomplete="new-password"
          onsubmit={() => void changePassword()}
        />
      </FormRow>
      <div class="row inline">
        <Button disabled={!canChange} onclick={() => void changePassword()}>{t('Change')}</Button>
      </div>
    </FormSection>
  {/if}

  <FormSection title={t('Embedded RDP')}>
    <FormRow label={engine?.name ?? 'FreeRDP'}>
      <span class="value selectable" data-testid="rdp-engine-version">{engine?.version ?? '—'}</span>
    </FormRow>
  </FormSection>

  {#if platform === 'macos'}
    <FormSection title={t('RDP password autofill')}>
      <FormRow label={t('Accessibility permission')}>
        {#if accessibility === null}
          <span class="value">—</span>
        {:else}
          <span class="status" class:granted={accessibility} data-testid="accessibility">
            {#if accessibility}<CircleCheck size={14} />{t('Granted')}{:else}<CircleX size={14} />{t(
                'Missing',
              )}{/if}
          </span>
        {/if}
      </FormRow>
      <div class="row inline">
        <Button onclick={() => void openAccessibility()}>{t('Open System Settings')}</Button>
        <Button onclick={() => void checkAccessibility()}>{t('Check')}</Button>
      </div>
      <p class="caption row">{t('Without it, the password is copied to the clipboard for 30 seconds.')}</p>
    </FormSection>
  {/if}
</div>

<style>
  .row {
    padding: 7px 0;
  }
  .row :global(.toggle) {
    width: 100%;
  }
  .inline {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .spacer {
    flex: 1;
  }
  .caption {
    margin: 0;
    color: var(--fg-secondary);
    font-size: var(--font-size-small);
  }
  .path {
    overflow-wrap: anywhere;
  }
  .value {
    color: var(--fg-secondary);
  }
  .warning {
    color: var(--danger);
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--danger);
  }
  .status.granted {
    color: var(--success);
  }
</style>
