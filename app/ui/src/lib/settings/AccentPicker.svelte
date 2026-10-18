<script lang="ts">
  import { t } from '../i18n/index.svelte';
  import {
    ACCENTS,
    appearance as defaultAppearance,
    type AppearanceStore,
  } from '../stores/appearance.svelte';

  interface Props {
    appearance?: Pick<AppearanceStore, 'accent' | 'setAccent'>;
  }

  let { appearance = defaultAppearance }: Props = $props();
</script>

<div class="accents" role="radiogroup" aria-label={t('Accent color')}>
  {#each ACCENTS as accent (accent.id)}
    <button
      type="button"
      role="radio"
      class="swatch"
      class:selected={appearance.accent.id === accent.id}
      aria-checked={appearance.accent.id === accent.id}
      aria-label={t(accent.name)}
      title={t(accent.name)}
      data-accent={accent.id}
      style:--swatch="oklch(0.62 {accent.chroma}
      {accent.hue})"
      style:--swatch-2="oklch(0.62 {accent.chroma}
      {accent.hue + 55})"
      onclick={() => appearance.setAccent(accent.id)}
    ></button>
  {/each}
</div>

<style>
  .accents {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .swatch {
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--swatch), var(--swatch-2));
    box-shadow:
      inset 0 0.5px 0 rgba(255, 255, 255, 0.35),
      0 0 0 0.5px rgba(0, 0, 0, 0.2);
    cursor: default;
    transition: transform 0.12s ease;
  }
  .swatch:hover {
    transform: scale(1.1);
  }
  .swatch.selected {
    box-shadow:
      0 0 0 2px var(--bg-elevated),
      0 0 0 3.5px var(--swatch);
  }
</style>
