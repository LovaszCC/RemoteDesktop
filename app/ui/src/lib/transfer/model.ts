// Pure helpers of the export and import sheets (immutable sets, so the stores' `$state` sees changes).

import { parseTag } from '../stores/library.svelte';
import type { EntryDto, LibraryDto, TransferAction, TransferDecisionDto, TransferRowDto } from '../types';

/** The export sheet's entries under one folder path (`''` is the root). */
export interface ExportGroup {
  path: string;
  entries: EntryDto[];
}

/** Entries grouped by folder path, groups and entries in name order, the root first. */
export function exportGroups(library: LibraryDto): ExportGroup[] {
  const groups = new Map<string, EntryDto[]>();
  for (const entry of library.entries) {
    const list = groups.get(entry.folderPath) ?? [];
    list.push(entry);
    groups.set(entry.folderPath, list);
  }
  const byName = (a: string, b: string) =>
    a.localeCompare(b, undefined, { numeric: true, sensitivity: 'base' });
  return [...groups.entries()]
    .sort(([a], [b]) => (a === '' ? -1 : b === '' ? 1 : byName(a, b)))
    .map(([path, entries]) => ({ path, entries: [...entries].sort((a, b) => byName(a.name, b.name)) }));
}

/** `set` with `ids` checked or unchecked. */
export function withAll(set: ReadonlySet<string>, ids: readonly string[], on: boolean): ReadonlySet<string> {
  const next = new Set(set);
  for (const id of ids) {
    if (on) next.add(id);
    else next.delete(id);
  }
  return next;
}

/** `all`, `none` or `some` of `ids` are in `set`. */
export function groupState(set: ReadonlySet<string>, ids: readonly string[]): 'all' | 'none' | 'some' {
  const count = ids.filter((id) => set.has(id)).length;
  if (count === 0) return 'none';
  return count === ids.length ? 'all' : 'some';
}

/** Ids of the folder and every folder below it. */
function subtree(library: LibraryDto, folderId: string): Set<string> {
  const ids = new Set([folderId]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const folder of library.folders) {
      if (folder.parentId && ids.has(folder.parentId) && !ids.has(folder.id)) {
        ids.add(folder.id);
        grew = true;
      }
    }
  }
  return ids;
}

/** What the export sheet starts with: the entries and credentials behind the sidebar selection. */
export function initialSelection(
  library: LibraryDto,
  tags: readonly string[],
): { entries: ReadonlySet<string>; credentials: ReadonlySet<string> } {
  const entries = new Set<string>();
  const credentials = new Set<string>();
  for (const tag of tags) {
    const parsed = parseTag(tag);
    if (!parsed) continue;
    switch (parsed.kind) {
      case 'entry':
      case 'favorite':
        entries.add(parsed.id);
        break;
      case 'connection': {
        const connection = library.connections.find((c) => c.id === parsed.id);
        if (connection) entries.add(connection.entryId);
        break;
      }
      case 'folder': {
        const folders = subtree(library, parsed.id);
        for (const entry of library.entries) {
          if (entry.folderId && folders.has(entry.folderId)) entries.add(entry.id);
        }
        break;
      }
      case 'credential':
        credentials.add(parsed.id);
        break;
      default:
        break;
    }
  }
  return { entries, credentials };
}

/** Ids of the credentials an entry's connections link to. */
export function linkedCredentials(library: LibraryDto, entryId: string): string[] {
  const ids: string[] = [];
  for (const connection of library.connections) {
    if (connection.entryId === entryId && connection.credentialId && !ids.includes(connection.credentialId)) {
      ids.push(connection.credentialId);
    }
  }
  return ids;
}

/** Credentials that checked entries link to but that are not checked: those entries go without them. */
export function missingCredentials(
  library: LibraryDto,
  entries: ReadonlySet<string>,
  credentials: ReadonlySet<string>,
): string[] {
  const missing: string[] = [];
  for (const id of entries) {
    for (const credential of linkedCredentials(library, id)) {
      if (!credentials.has(credential) && !missing.includes(credential)) missing.push(credential);
    }
  }
  return missing;
}

/** Why the export can't start yet: `null` when it can. */
export function exportProblem(
  entries: number,
  credentials: number,
  password: string,
  confirm: string,
): 'nothing' | 'mismatch' | null {
  if (entries + credentials === 0) return 'nothing';
  if (password !== confirm) return 'mismatch';
  return null;
}

/** What a checked conflict row does; `null` until the user chooses. */
export type ConflictChoice = Exclude<TransferAction, 'skip'> | null;

/** New rows start checked; conflicting ones too, but without a choice. */
export function initialImportIncluded(rows: readonly TransferRowDto[]): ReadonlySet<string> {
  return new Set(rows.map((r) => r.id));
}

/** Checked conflict rows still waiting for a choice. */
export function unanswered(
  rows: readonly TransferRowDto[],
  included: ReadonlySet<string>,
  choices: Readonly<Record<string, ConflictChoice>>,
): number {
  return rows.filter((r) => r.conflict && included.has(r.id) && !choices[r.id]).length;
}

/** One decision per row: unchecked rows are skipped, new ones created, conflicts as chosen. */
export function decisions(
  rows: readonly TransferRowDto[],
  included: ReadonlySet<string>,
  choices: Readonly<Record<string, ConflictChoice>>,
): TransferDecisionDto[] {
  return rows.map((row) => {
    if (!included.has(row.id)) return { id: row.id, action: 'skip' };
    if (!row.conflict) return { id: row.id, action: 'new' };
    return { id: row.id, action: choices[row.id] ?? 'skip' };
  });
}
