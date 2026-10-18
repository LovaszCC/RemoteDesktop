// Pure helpers of the import sheet's checkboxes (immutable sets, so the store's `$state` sees changes).

import type { ImportRowDto } from '../types';

/** New rows start checked unless they already exist (Swift `markDuplicates`). */
export function initialIncluded(rows: readonly ImportRowDto[]): ReadonlySet<string> {
  return new Set(rows.filter((r) => !r.duplicate).map((r) => r.id));
}

/** `included` with `id` checked or unchecked. */
export function withIncluded(included: ReadonlySet<string>, id: string, on: boolean): ReadonlySet<string> {
  const next = new Set(included);
  if (on) next.add(id);
  else next.delete(id);
  return next;
}

/** Select all / Select none. */
export function allIncluded(rows: readonly ImportRowDto[], on: boolean): ReadonlySet<string> {
  return new Set(on ? rows.map((r) => r.id) : []);
}
