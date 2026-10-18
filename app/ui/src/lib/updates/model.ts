// Texts for the update dialog and the Settings "Updates" section.

import { t } from '../i18n/index.svelte';
import { formatBytes } from '../terminal/model';
import type { UpdatePhase } from './updates.svelte';

/** One line for Settings next to "Check now"; `null` before the first check. */
export function updateStatusText(phase: UpdatePhase): string | null {
  switch (phase.kind) {
    case 'idle':
      return null;
    case 'checking':
      return t('Checking for updates…');
    case 'notConfigured':
      return t("Updates aren't set up in this build.");
    case 'upToDate':
      return t('RemoteDesktop is up to date.');
    case 'available':
    case 'installing':
      return t('RemoteDesktop %@ is available.', phase.update.version);
    case 'installed':
      return t('The update is installed.');
    case 'failed':
      return phase.message;
  }
}

/** Release notes as plain text: GitHub Markdown kept line by line, heading marks dropped (Swift). */
export function notesText(markdown: string | null): string {
  return (markdown ?? '')
    .split('\n')
    .map((line) => line.replace(/^#+\s*/, ''))
    .join('\n')
    .trim();
}

/** Download progress: `2 MB / 10 MB`, or just the downloaded size while the total is unknown. */
export function progressText(downloaded: number, total: number | null, locale: string): string {
  const done = formatBytes(downloaded, locale);
  return total ? t('%@ of %@', done, formatBytes(total, locale)) : done;
}

/** 0…1 for a `<progress>`, or `null` (indeterminate) without a total. */
export function progressFraction(downloaded: number, total: number | null): number | null {
  return total ? Math.min(1, downloaded / total) : null;
}
