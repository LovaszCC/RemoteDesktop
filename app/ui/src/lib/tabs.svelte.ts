// The detail area's tabs (Swift `SessionTabs`): the fixed Details tab plus one tab per session.
// `active === null` means Details. Colour overrides distinguish "explicitly no colour" (`null` value) from
// "automatic" (no key); custom titles are trimmed and an empty one restores the automatic title.
//
// `TabsStore` adds what the tab bar shows for each session (automatic title, type, colour, root warning).
// Whoever owns a session (the embedded SSH sessions store) hears about closed tabs through
// `onSessionClosed`, whichever way the tab was closed.

import type { ConnectionType, LabelColor } from './types';

export class SessionTabs {
  ids = $state<string[]>([]);
  active = $state<string | null>(null);
  /** Colours chosen for open tabs; a `null` value means explicitly no colour. */
  colorOverrides = $state<Record<string, LabelColor | null>>({});
  /** Names the user gave to open tabs. */
  customTitles = $state<Record<string, string>>({});

  /** Adds a session tab (once), right after `after` when that tab is open, else at the end; activates it. */
  open(id: string, after?: string) {
    if (!this.ids.includes(id)) {
      const index = after === undefined ? -1 : this.ids.indexOf(after);
      this.ids = index < 0 ? [...this.ids, id] : insertAt(this.ids, index + 1, id);
    }
    this.active = id;
  }

  /** Removes a tab; closing the active one selects its right neighbour, else the left, else Details. */
  close(id: string) {
    const index = this.ids.indexOf(id);
    if (index < 0) return;
    this.ids = this.ids.filter((x) => x !== id);
    this.colorOverrides = without(this.colorOverrides, id);
    this.customTitles = without(this.customTitles, id);
    if (this.active !== id) return;
    this.active = this.ids.length === 0 ? null : (this.ids[Math.min(index, this.ids.length - 1)] ?? null);
  }

  /** Puts `next` in place of `old` (same position, keeps active, colour and title). Unknown ids are ignored. */
  replace(old: string, next: string) {
    const index = this.ids.indexOf(old);
    if (index < 0 || this.ids.includes(next)) return;
    this.ids = this.ids.map((x) => (x === old ? next : x));
    if (this.active === old) this.active = next;
    if (old in this.colorOverrides) {
      const color = this.colorOverrides[old] ?? null;
      this.colorOverrides = { ...without(this.colorOverrides, old), [next]: color };
    }
    if (old in this.customTitles) {
      const title = this.customTitles[old] ?? '';
      this.customTitles = { ...without(this.customTitles, old), [next]: title };
    }
  }

  /** Renames a tab; an empty name goes back to the automatic title. Unknown ids are ignored. */
  rename(id: string, title: string) {
    if (!this.ids.includes(id)) return;
    const trimmed = title.trim();
    this.customTitles =
      trimmed === '' ? without(this.customTitles, id) : { ...this.customTitles, [id]: trimmed };
  }

  /** The user's name for the tab, else `automatic`. */
  title(id: string, automatic: string): string {
    return this.customTitles[id] ?? automatic;
  }

  /** Sets a tab's colour (`null`: explicitly none). Unknown ids are ignored. */
  setColor(color: LabelColor | null, id: string) {
    if (!this.ids.includes(id)) return;
    this.colorOverrides = { ...this.colorOverrides, [id]: color };
  }

  /** Back to the automatic colour. */
  resetColor(id: string) {
    this.colorOverrides = without(this.colorOverrides, id);
  }

  /** The override if set, else `automatic`. */
  color(id: string, automatic: LabelColor | null): LabelColor | null {
    return id in this.colorOverrides ? (this.colorOverrides[id] ?? null) : automatic;
  }

  /** Selects a session tab, or Details with `null`. Unknown ids are ignored. */
  select(id: string | null) {
    if (id !== null && !this.ids.includes(id)) return;
    this.active = id;
  }

  /** Cycles through [Details, …sessions], wrapping around. */
  cycle(forward: boolean) {
    const all: (string | null)[] = [null, ...this.ids];
    const current = Math.max(0, all.indexOf(this.active));
    this.active = all[(current + (forward ? 1 : -1) + all.length) % all.length] ?? null;
  }
}

function insertAt<T>(list: readonly T[], index: number, item: T): T[] {
  return [...list.slice(0, index), item, ...list.slice(index)];
}

function without<T>(record: Record<string, T>, key: string): Record<string, T> {
  if (!(key in record)) return record;
  const rest = { ...record };
  delete rest[key];
  return rest;
}

/** What the tab bar needs to know about a session. */
export interface TabSession {
  id: string;
  /** The automatic title (connection name, terminal title). */
  title: string;
  type: ConnectionType;
  /** The automatic colour: the connection's (or, for a split, the one every pane shares). */
  color: LabelColor | null;
  /** A root shell runs in it: warning icon and orange tint. */
  root?: boolean;
  /** A split tab (2–4 terminal panes). */
  split?: boolean;
  /** A split tab with a broadcasting pane: orange tint. */
  broadcasting?: boolean;
  /** The connection the tab belongs to: its tabs sit next to each other and are numbered. */
  group?: string;
  /** Set by `openSession` for a grouped tab: 2 and up add ` (2)`… to the automatic title. */
  ordinal?: number;
}

export class TabsStore extends SessionTabs {
  sessions = $state<TabSession[]>([]);
  // Not state: listeners don't render.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  #closedListeners = new Set<(id: string) => void>();

  /** Calls `listener` with the id of every closed session tab; returns the unsubscribe. */
  onSessionClosed(listener: (id: string) => void): () => void {
    this.#closedListeners.add(listener);
    return () => this.#closedListeners.delete(listener);
  }

  /** Changes what the tab bar shows for a session. Unknown ids are ignored. */
  updateSession(id: string, patch: Partial<Omit<TabSession, 'id'>>) {
    const index = this.sessions.findIndex((s) => s.id === id);
    const current = this.sessions[index];
    if (!current) return;
    const keys = Object.keys(patch) as (keyof typeof patch)[];
    if (keys.every((k) => current[k] === patch[k])) return;
    this.sessions = this.sessions.map((s, i) => (i === index ? { ...s, ...patch } : s));
  }

  /** The active session, or `null` on Details. */
  get activeSession(): TabSession | null {
    return this.sessions.find((s) => s.id === this.active) ?? null;
  }

  get hasSessions(): boolean {
    return this.sessions.length > 0;
  }

  session(id: string): TabSession | undefined {
    return this.sessions.find((s) => s.id === id);
  }

  /**
   * Adds a session and activates its tab. A grouped one opens right after the group's last tab and takes
   * the lowest number free among the group's tabs with the same title; anything else (or the group's first
   * open tab) goes right after the open tab `after`, else to the end.
   */
  openSession(session: TabSession, after?: string) {
    if (this.session(session.id)) return this.open(session.id);
    const group = session.group === undefined ? [] : this.sessions.filter((s) => s.group === session.group);
    const last = group.at(-1) ?? this.session(after ?? '');
    let next = session;
    if (session.group !== undefined) {
      const used = group.filter((s) => s.title === session.title).map((s) => s.ordinal ?? 1);
      let ordinal = 1;
      while (used.includes(ordinal)) ordinal += 1;
      next = { ...session, ordinal };
    }
    const index = last ? this.sessions.indexOf(last) : -1;
    this.sessions = index < 0 ? [...this.sessions, next] : insertAt(this.sessions, index + 1, next);
    this.open(session.id, last?.id);
  }

  /**
   * Puts `next` in the place of session `old` (position, active, colour and title kept) without closing it:
   * nobody hears about a closed tab. Unknown `old` ids are ignored.
   */
  replaceSession(old: string, next: TabSession) {
    const current = this.session(old);
    if (!current || this.session(next.id)) return;
    // A reconnect keeps the tab's group and number; turning it into a split leaves the group.
    const kept =
      next.split || next.group !== undefined || current.group === undefined
        ? next
        : { ...next, group: current.group, ordinal: current.ordinal };
    this.sessions = this.sessions.map((s) => (s.id === old ? kept : s));
    this.replace(old, next.id);
  }

  /**
   * Removes a session's tab without closing the session: nobody hears about a closed tab (its session lives
   * on as a split pane). Unknown ids are ignored.
   */
  detachSession(id: string) {
    if (!this.session(id)) return;
    this.sessions = this.sessions.filter((s) => s.id !== id);
    this.close(id);
  }

  /** Closes a session and its tab. */
  closeSession(id: string) {
    if (!this.session(id)) return;
    this.sessions = this.sessions.filter((s) => s.id !== id);
    this.close(id);
    for (const listener of [...this.#closedListeners]) listener(id);
  }

  /** "Close session": the active one; nothing on Details. */
  closeActive() {
    if (this.active !== null) this.closeSession(this.active);
  }

  /** The shown title of a session tab: the user's name, else the automatic one with its number. */
  displayTitle(session: TabSession): string {
    const ordinal = session.ordinal ?? 1;
    return this.title(session.id, ordinal > 1 ? `${session.title} (${ordinal})` : session.title);
  }

  /** The shown colour of a session tab. */
  displayColor(session: TabSession): LabelColor | null {
    return this.color(session.id, session.color);
  }
}

export function createTabsStore() {
  return new TabsStore();
}

export const tabs = createTabsStore();

let testTabCount = 0;

/** Dev builds only: opens a fake session so the tab bar can be exercised before sessions exist. */
export function openTestTab(store: TabsStore = tabs) {
  testTabCount += 1;
  const types: ConnectionType[] = ['ssh', 'rdp', 'web', 'mikrotik'];
  const colors: (LabelColor | null)[] = [null, 'red', 'blue', 'green'];
  store.openSession({
    id: `test-${Date.now()}-${testTabCount}`,
    title: `Test session ${testTabCount}`,
    type: types[testTabCount % types.length] ?? 'ssh',
    color: colors[testTabCount % colors.length] ?? null,
  });
}
