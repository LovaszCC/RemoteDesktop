/** An immutable id → item lookup (rebuilt whenever the library changes). */
export function indexById<T extends { id: string }>(items: readonly T[]): ReadonlyMap<string, T> {
  return new Map(items.map((item) => [item.id, item]));
}

/** `items` without repeats, first occurrence wins. */
export function unique<T>(items: Iterable<T>): T[] {
  return [...new Set(items)];
}
