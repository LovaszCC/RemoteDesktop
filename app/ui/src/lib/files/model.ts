// Pure helpers of the Files window: sorting, formatting, paths, selection and transfer texts.

import { FAILURE_KEYS } from '../forwards/model';
import { t, tn } from '../i18n/index.svelte';
import type { FileEntryDto, FilesDoneDto, FilesProgressDto, FilesStatusDto } from '../types';

/** An OS drag over the window: logical client coordinates, and the paths on drop. */
export type OsDrop =
  | { type: 'over'; x: number; y: number }
  | { type: 'drop'; x: number; y: number; paths: string[] }
  | { type: 'leave' };

export type SortKey = 'name' | 'size' | 'modified' | 'permissions';

export interface SortOrder {
  key: SortKey;
  ascending: boolean;
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** Finder order for names (Swift `localizedStandardCompare`): case and accents ignored, numbers numeric. */
export function compareNames(a: string, b: string): number {
  return collator.compare(a, b) || (a < b ? -1 : a > b ? 1 : 0);
}

/** Folders first, then by the column; ties by name. */
export function sortEntries(entries: readonly FileEntryDto[], order: SortOrder): FileEntryDto[] {
  const direction = order.ascending ? 1 : -1;
  const column = (a: FileEntryDto, b: FileEntryDto): number => {
    switch (order.key) {
      case 'size':
        return a.size - b.size;
      case 'modified':
        return (a.modified ?? -Infinity) - (b.modified ?? -Infinity);
      case 'permissions':
        return a.permissions < b.permissions ? -1 : a.permissions > b.permissions ? 1 : 0;
      default:
        return compareNames(a.name, b.name);
    }
  };
  return [...entries].sort((a, b) => {
    const folders = Number(isFolder(b)) - Number(isFolder(a));
    if (folders !== 0) return folders;
    return direction * (column(a, b) || compareNames(a.name, b.name));
  });
}

/** Clicking a header: the same column flips, another one starts ascending. */
export function nextSort(order: SortOrder, key: SortKey): SortOrder {
  return order.key === key ? { key, ascending: !order.ascending } : { key, ascending: true };
}

export function isFolder(entry: FileEntryDto): boolean {
  return entry.kind === 'directory';
}

/** Folders open; so do remote symlinks (they may point to a folder; the listing says if not). */
export function isOpenable(entry: FileEntryDto): boolean {
  return entry.kind === 'directory' || entry.kind === 'symlink';
}

/** `ByteCountFormatter` `.file` style: 1000-based, one decimal. */
export function formatSize(bytes: number, locale: string): string {
  if (bytes < 1000) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB', 'TB', 'PB'];
  let value = bytes / 1000;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const number = new Intl.NumberFormat(locale, { maximumFractionDigits: value < 10 ? 1 : 0 }).format(value);
  return `${number} ${units[unit]}`;
}

/** Date and time of Unix seconds, in the UI language. */
export function formatDate(seconds: number | null, locale: string): string {
  if (seconds === null) return '';
  return new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'short' }).format(
    new Date(seconds * 1000),
  );
}

/** POSIX `directory/name`. */
export function joinRemote(directory: string, name: string): string {
  return directory.endsWith('/') ? directory + name : `${directory}/${name}`;
}

/** A local `directory` + `name` with the folder's own separator (`\` for Windows paths). */
export function joinLocal(directory: string, name: string): string {
  const separator = directory.includes('\\') && !directory.includes('/') ? '\\' : '/';
  return directory.endsWith(separator) ? directory + name : directory + separator + name;
}

/** The last component of a local or remote path. */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter((p) => p.length > 0);
  return parts[parts.length - 1] ?? path;
}

/** The folder holding a local or remote path. */
export function dirName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, '');
  const cut = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  if (cut < 0) return trimmed;
  if (cut === 0) return trimmed[0] ?? '/';
  // `C:\x` → `C:\`.
  if (cut === 2 && trimmed[1] === ':') return trimmed.slice(0, 3);
  return trimmed.slice(0, cut);
}

/** Local paths grouped by their folder (OS drops can come from anywhere), in first-seen order. */
export function groupByFolder(paths: readonly string[]): { folder: string; paths: string[] }[] {
  const groups = new Map<string, string[]>();
  for (const path of paths) {
    const folder = dirName(path);
    groups.set(folder, [...(groups.get(folder) ?? []), path]);
  }
  return [...groups].map(([folder, list]) => ({ folder, paths: list }));
}

export interface ClickModifiers {
  /** ⌘ on macOS, Ctrl elsewhere: toggle one row. */
  toggle: boolean;
  /** Shift: a range from the anchor. */
  range: boolean;
}

/** The selection after clicking `name` (rows in display order). */
export function clickSelection(
  selection: readonly string[],
  rows: readonly string[],
  name: string,
  anchor: string | null,
  modifiers: ClickModifiers,
): { selection: string[]; anchor: string } {
  if (modifiers.range && anchor !== null && rows.includes(anchor)) {
    const a = rows.indexOf(anchor);
    const b = rows.indexOf(name);
    return { selection: rows.slice(Math.min(a, b), Math.max(a, b) + 1), anchor };
  }
  if (modifiers.toggle) {
    const next = selection.includes(name) ? selection.filter((n) => n !== name) : [...selection, name];
    return { selection: next, anchor: name };
  }
  return { selection: [name], anchor: name };
}

/** The bar's fraction (0…1), `null` while the size is unknown. Capped below 1 until done. */
export function progressFraction(progress: FilesProgressDto): number | null {
  if (!progress.bytesTotal) return null;
  return Math.min(progress.bytesDone / progress.bytesTotal, 0.99);
}

/** "Downloading “name” (2 of 5)". */
export function progressTitle(progress: FilesProgressDto): string {
  const key = progress.direction === 'download' ? 'Downloading “%@”' : 'Uploading “%@”';
  const title = t(key, progress.name);
  return progress.count > 1 ? `${title} (${t('%lld of %lld', progress.index + 1, progress.count)})` : title;
}

/** "1.2 MB of 3.4 MB" or "1.2 MB". */
export function progressBytes(progress: FilesProgressDto, locale: string): string {
  const done = formatSize(progress.bytesDone, locale);
  return progress.bytesTotal ? t('%@ of %@', done, formatSize(progress.bytesTotal, locale)) : done;
}

/** The result line of a finished transfer. */
export function doneSummary(done: FilesDoneDto): string {
  if (done.cancelled) return t('Cancelled');
  const count = (status: string) => done.items.filter((i) => i.status === status).length;
  const failed = count('failed');
  const skipped = count('skipped');
  const copied = count('done');
  const parts: string[] = [];
  if (copied > 0) parts.push(tn('%lld copied', copied));
  if (skipped > 0) parts.push(tn('%lld skipped', skipped));
  if (failed > 0) parts.push(tn('%lld failed', failed));
  return parts.join(', ') || t('Done');
}

/** The failed items with their (translated where known) errors. */
export function doneErrors(done: FilesDoneDto): string[] {
  return done.items
    .filter((i) => i.status === 'failed')
    .map((i) => `${i.name}: ${transferError(i.error ?? '')}`);
}

const TRANSFER_ERROR_KEYS = new Set(['Folders need tar on the server.', 'The transfer failed.']);

function transferError(error: string): string {
  return TRANSFER_ERROR_KEYS.has(error) ? t(error) : error;
}

/** The connection failure for the window, translated by code; the English text for unknown codes. */
export function connectFailure(status: FilesStatusDto): string {
  if (status.state !== 'failed') return '';
  const override: Record<string, string> = {
    proxyUnsupported: 'ProxyJump and ProxyCommand are not supported for the file browser.',
    hostCertificate: 'Host certificates are not supported for the file browser.',
  };
  const key = override[status.code] ?? FAILURE_KEYS[status.code];
  return key && !key.includes('%@') ? t(key) : status.error;
}
