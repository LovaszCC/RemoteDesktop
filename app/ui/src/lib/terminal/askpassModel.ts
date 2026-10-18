import { t } from '../i18n/index.svelte';
import type { AskpassKind } from '../types';

/** The askpass dialog's title per prompt kind. */
export function askpassTitle(kind: AskpassKind): string {
  switch (kind) {
    case 'password':
      return t('Password');
    case 'passphrase':
      return t('Key passphrase');
    case 'confirm':
      return t('Unknown host key');
    case 'other':
      return 'SSH';
  }
}
