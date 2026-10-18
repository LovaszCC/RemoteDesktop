// Small text helpers for the views.

/** Swift `.truncationMode(.middle)`: `/Users/me/…/connections.sqlite` when longer than `max`. */
export function middleTruncate(text: string, max = 64): string {
  if (text.length <= max) return text;
  const keep = max - 1;
  const head = Math.ceil(keep / 2);
  const tail = keep - head;
  return `${text.slice(0, head)}…${text.slice(text.length - tail)}`;
}

/** "5 minutes ago" / "5 perccel ezelőtt" for an age in seconds. */
export function relativeAge(seconds: number, locale: string): string {
  const format = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return format.format(-s, 'second');
  if (s < 3600) return format.format(-Math.round(s / 60), 'minute');
  if (s < 86400) return format.format(-Math.round(s / 3600), 'hour');
  return format.format(-Math.round(s / 86400), 'day');
}
