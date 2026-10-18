<script lang="ts">
  // Swift `ShortcutsSettingsView`: every command with a recorder. While recording, key presses are
  // swallowed; Esc cancels, ⌫ alone removes, a refused shortcut shows the error and recording goes on.
  import Undo2 from '@lucide/svelte/icons/undo-2';
  import { onDestroy } from 'svelte';
  import Button from '../components/Button.svelte';
  import FormSection from '../editors/FormSection.svelte';
  import { commandTitle, errorMessage } from '../errors';
  import { t } from '../i18n/index.svelte';
  import { recorder as defaultRecorder, type RecorderStore } from '../shortcuts/recorder.svelte';
  import { app as defaultApp, type AppStore } from '../stores/app.svelte';
  import { settings as defaultSettings, type SettingsStore } from '../stores/settings.svelte';
  import type { ShortcutCommand, ShortcutDto } from '../types';
  import { recorderAction, recorderCaption, shortcutGroups } from './model';

  interface Props {
    app?: Pick<AppStore, 'platform'>;
    settings?: SettingsStore;
    recorder?: RecorderStore;
  }

  let { app = defaultApp, settings = defaultSettings, recorder = defaultRecorder }: Props = $props();

  const groups = $derived(shortcutGroups(settings.shortcuts));

  onDestroy(() => {
    recorder.stop();
    recorder.clearErrors();
  });

  /** Applies a change; success ends recording, a refusal shows the error and recording goes on. */
  async function perform(command: ShortcutCommand, change: () => Promise<unknown>) {
    try {
      await change();
      recorder.setError(command, null);
      if (recorder.recording === command) recorder.stop();
    } catch (error) {
      recorder.setError(command, errorMessage(error));
    }
  }

  // Swallows every key press while a recorder listens, before anything else sees it.
  $effect(() => {
    const command = recorder.recording;
    if (command === null) return;
    const keydown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      const action = recorderAction(event);
      switch (action.kind) {
        case 'cancel':
          recorder.stop();
          break;
        case 'remove':
          void perform(command, () => settings.setShortcut(command, null));
          break;
        case 'set':
          void perform(command, () => settings.setShortcut(command, action.shortcut));
          break;
        case 'ignore':
          break;
      }
    };
    window.addEventListener('keydown', keydown, true);
    return () => window.removeEventListener('keydown', keydown, true);
  });

  function resetAll() {
    recorder.stop();
    recorder.clearErrors();
    void settings.resetAllShortcuts().catch(() => {});
  }
</script>

{#snippet row(shortcut: ShortcutDto)}
  {@const command = shortcut.command}
  {@const recording = recorder.recording === command}
  {@const error = recorder.error(command)}
  <div class="row">
    <div class="line">
      <span class="title">{commandTitle(command)}</span>
      {#if shortcut.customized}
        <button
          type="button"
          class="reset"
          aria-label={t('Restore default (%@)', shortcut.defaultDisplay)}
          title={t('Restore default (%@)', shortcut.defaultDisplay)}
          onclick={() => void perform(command, () => settings.resetShortcut(command))}
        >
          <Undo2 size={13} />
        </button>
      {/if}
      <button
        type="button"
        class="recorder"
        class:recording
        aria-pressed={recording}
        data-command={command}
        onclick={() => recorder.toggle(command)}
      >
        {#if recording}
          {t('Press keys…')}
        {:else if shortcut.display}
          {shortcut.display}
        {:else}
          <span class="none">{t('None')}</span>
        {/if}
      </button>
    </div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
{/snippet}

<div class="shortcuts">
  {#each groups as group (group.title)}
    <FormSection title={group.title}>
      {#each group.rows as shortcut (shortcut.command)}
        {@render row(shortcut)}
      {/each}
    </FormSection>
  {/each}
  <div class="footer">
    <p class="caption">{recorderCaption(app.platform)}</p>
    <Button disabled={settings.customizedCount === 0} onclick={resetAll}>{t('Restore defaults')}</Button>
  </div>
</div>

<style>
  .row {
    padding: 5px 0;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .title {
    flex: 1;
    min-width: 0;
  }
  .reset {
    display: inline-flex;
    padding: 2px;
    border: none;
    border-radius: 4px;
    background: none;
    color: var(--fg-secondary);
  }
  .reset:hover {
    color: var(--fg);
  }
  .recorder {
    min-width: 90px;
    min-height: var(--control-height);
    padding: 1px 10px;
    border: 0.5px solid var(--border-control);
    border-radius: var(--radius);
    background: var(--bg-control);
    font-variant-numeric: tabular-nums;
  }
  .recorder.recording {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent);
  }
  .none {
    color: var(--fg-secondary);
  }
  .error {
    margin: 4px 0 0;
    color: var(--danger);
    font-size: var(--font-size-small);
  }
  .footer {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .caption {
    flex: 1;
    margin: 0;
    color: var(--fg-secondary);
    font-size: var(--font-size-small);
  }
</style>
