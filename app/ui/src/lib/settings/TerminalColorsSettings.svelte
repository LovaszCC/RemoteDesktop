<script lang="ts">
  // Settings → Terminal colors: one set for the app's dark mode and one for light, every colour on its
  // own picker, with a live preview and a reset to the Catppuccin defaults.
  import Button from '../components/Button.svelte';
  import Segmented from '../components/Segmented.svelte';
  import FormSection from '../editors/FormSection.svelte';
  import { t } from '../i18n/index.svelte';
  import { appearance as defaultAppearance, type AppearanceStore } from '../stores/appearance.svelte';
  import { terminalPalette, type TerminalColorKey, type TerminalMode } from '../terminal/colors';

  interface Props {
    appearance?: Pick<AppearanceStore, 'terminalColors' | 'setTerminalColor' | 'resetTerminalColors'>;
    /** The set shown first; the mode in use by default. */
    initialMode?: TerminalMode;
  }

  let {
    appearance = defaultAppearance,
    initialMode = globalThis.document?.documentElement.dataset.theme === 'dark' ? 'dark' : 'light',
  }: Props = $props();

  // svelte-ignore state_referenced_locally
  let mode = $state<TerminalMode>(initialMode);
  const palette = $derived(terminalPalette(mode, appearance.terminalColors));
  const changed = $derived(Object.keys(appearance.terminalColors[mode]).length > 0);

  const ANSI = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white'] as const;
  const capitalized = (name: string) => name[0]!.toUpperCase() + name.slice(1);
  const groups: { title: string; keys: { key: TerminalColorKey; label: string }[] }[] = [
    {
      title: 'Basic',
      keys: [
        { key: 'background', label: 'Background' },
        { key: 'foreground', label: 'Foreground' },
        { key: 'cursor', label: 'Cursor' },
        { key: 'selectionBackground', label: 'Selection' },
      ],
    },
    { title: 'Normal', keys: ANSI.map((name) => ({ key: name, label: capitalized(name) })) },
    {
      title: 'Bright',
      keys: ANSI.map((name) => ({
        key: `bright${capitalized(name)}` as TerminalColorKey,
        label: capitalized(name),
      })),
    },
  ];

  /** The picker works on `#rrggbb`; an alpha the colour had (selection) is kept. */
  function pick(key: TerminalColorKey, rgb: string) {
    const alpha = palette[key].length === 9 ? palette[key].slice(7) : '';
    appearance.setTerminalColor(mode, key, `${rgb}${alpha}`);
  }
</script>

<FormSection title={t('Terminal colors')}>
  <div class="colors">
    <div class="top">
      <Segmented
        value={mode}
        options={[
          { value: 'dark', label: t('Dark') },
          { value: 'light', label: t('Light') },
        ]}
        label={t('Terminal colors')}
        onchange={(value) => (mode = value)}
      />
      <Button disabled={!changed} onclick={() => appearance.resetTerminalColors(mode)}>
        {mode === 'dark' ? t('Reset to Catppuccin Mocha') : t('Reset to Catppuccin Latte')}
      </Button>
    </div>
    <div
      class="preview"
      style:background={palette.background}
      style:color={palette.foreground}
      aria-hidden="true"
    >
      <span>user@host:~$ ls</span>
      <span class="swatches">
        {#each groups.slice(1) as group (group.title)}
          {#each group.keys as { key } (key)}
            <span class="swatch" style:background={palette[key]}></span>
          {/each}
        {/each}
      </span>
    </div>
    {#each groups as group (group.title)}
      <div class="group">
        <span class="group-title">{t(group.title)}</span>
        <div class="grid">
          {#each group.keys as { key, label } (key)}
            <label class="color">
              <input
                type="color"
                data-key={key}
                value={palette[key].slice(0, 7)}
                onchange={(e) => pick(key, e.currentTarget.value)}
              />
              <span>{t(label)}</span>
            </label>
          {/each}
        </div>
      </div>
    {/each}
    <p class="hint">{t('Changes apply to open terminals at once.')}</p>
  </div>
</FormSection>

<style>
  .colors {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 4px 0;
  }
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .preview {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 10px;
    border-radius: 6px;
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .swatches {
    display: inline-flex;
    gap: 3px;
  }
  .swatch {
    width: 10px;
    height: 10px;
    border-radius: 2px;
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .group-title {
    color: var(--fg-secondary);
    font-size: 11px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 6px 12px;
  }
  .color {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .color input {
    width: 26px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--separator);
    border-radius: 4px;
    background: none;
    cursor: pointer;
  }
  .hint {
    margin: 0;
    color: var(--fg-secondary);
    font-size: 11px;
  }
</style>
