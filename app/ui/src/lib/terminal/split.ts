// A split tab's pure rules (Swift `SplitLayout`, `BroadcastGroup`, `SplitSession.title`, `LabelColor.common`,
// `SavedSplit.connections(in:)`; the Rust ports live in `rd_core::split`). The tab holds 2–4 embedded SSH
// panes; typing into a broadcasting pane goes to every other broadcasting, running pane.

import type { MenuItem } from '../components/menu.svelte';
import { t } from '../i18n/index.svelte';
import type { ConnectionDto, LabelColor } from '../types';

/** The most panes a split tab holds. */
export const MAX_PANES = 4;

/** One pane: its embedded session (the Rust session id) and the connection it runs. */
export interface SplitPane {
  id: string;
  connectionId: string;
}

export interface SplitState {
  /** The tab id. */
  id: string;
  /** Panes in order (the grid fills by `splitColumns`). */
  panes: SplitPane[];
  /** Broadcasting pane ids. */
  broadcasting: string[];
  /** The pane that gets the keyboard focus. */
  focused: string | null;
  /** The saved split this tab was opened from or saved as. */
  savedSplitId: string | null;
}

/**
 * Pane indices per column, left to right and top to bottom: 1 → one column; 2 → side by side; 3 → one left,
 * two stacked right; 4 → 2×2 (row-major: 0 1 / 2 3). Counts above `MAX_PANES` are clamped.
 */
export function splitColumns(count: number): number[][] {
  switch (Math.min(count, MAX_PANES)) {
    case 1:
      return [[0]];
    case 2:
      return [[0], [1]];
    case 3:
      return [[0], [1, 2]];
    case 4:
      return [
        [0, 2],
        [1, 3],
      ];
    default:
      return [];
  }
}

export function newSplit(
  id: string,
  paneIds: readonly SplitPane[],
  savedSplitId: string | null = null,
): SplitState {
  const panes = paneIds.slice(0, MAX_PANES);
  return { id, panes, broadcasting: [], focused: panes[0]?.id ?? null, savedSplitId };
}

export const isFull = (split: SplitState) => split.panes.length >= MAX_PANES;

/** Adds a pane (not broadcasting, focused); `null` when the split is full or holds it already. */
export function withPane(split: SplitState, pane: SplitPane): SplitState | null {
  if (isFull(split) || split.panes.some((p) => p.id === pane.id)) return null;
  return { ...split, panes: [...split.panes, pane], focused: pane.id };
}

/** Removes a pane and its broadcast flag; the focus moves to the pane now at its place (or the last). */
export function withoutPane(split: SplitState, paneId: string): SplitState {
  const index = split.panes.findIndex((p) => p.id === paneId);
  if (index < 0) return split;
  const panes = split.panes.filter((p) => p.id !== paneId);
  const focused =
    split.focused === paneId ? (panes[Math.min(index, panes.length - 1)]?.id ?? null) : split.focused;
  return { ...split, panes, broadcasting: split.broadcasting.filter((id) => id !== paneId), focused };
}

/** Switches one pane's broadcast; unknown panes are ignored. */
export function withBroadcast(split: SplitState, paneId: string, on: boolean): SplitState {
  if (!split.panes.some((p) => p.id === paneId)) return split;
  const rest = split.broadcasting.filter((id) => id !== paneId);
  return { ...split, broadcasting: on ? [...rest, paneId] : rest };
}

/** Every pane broadcasts (and there is at least one). */
export function isAllBroadcasting(split: SplitState): boolean {
  return split.panes.length > 0 && split.panes.every((p) => split.broadcasting.includes(p.id));
}

/** "Broadcast to all panes": all on, or (when all already broadcast) all off. */
export function withAllBroadcastToggled(split: SplitState): SplitState {
  return { ...split, broadcasting: isAllBroadcasting(split) ? [] : split.panes.map((p) => p.id) };
}

/**
 * Panes that receive input typed into `source`: none unless it broadcasts, else the other broadcasting,
 * running panes in pane order.
 */
export function broadcastTargets(
  split: SplitState,
  source: string,
  isRunning: (id: string) => boolean,
): string[] {
  if (!split.broadcasting.includes(source)) return [];
  return split.panes
    .map((p) => p.id)
    .filter((id) => id !== source && split.broadcasting.includes(id) && isRunning(id));
}

/** The colour every item shares, else `null` (also for none). */
export function commonColor(colors: readonly (LabelColor | null)[]): LabelColor | null {
  const first = colors[0] ?? null;
  return colors.every((c) => c === first) ? first : null;
}

/** The automatic tab title: "a + b" for up to two panes, else "Split (n)". */
export function splitTitle(names: readonly string[]): string {
  return names.length <= 2 ? names.join(' + ') : t('Split (%lld)', names.length);
}

/** What a saved split opens: its existing connections in order, duplicates kept, at most `MAX_PANES`. */
export function savedSplitConnections(
  connectionIds: readonly string[],
  lookup: (id: string) => ConnectionDto | undefined,
): ConnectionDto[] {
  return connectionIds
    .map(lookup)
    .filter((c): c is ConnectionDto => c !== undefined)
    .slice(0, MAX_PANES);
}

/**
 * The "Add terminal" menu: the panes' connections again (each once, in pane order), then every SSH-capable
 * connection by name, then (with `addLocal`) a local terminal.
 */
export function addTerminalItems(
  split: SplitState,
  connections: readonly ConnectionDto[],
  add: (connection: ConnectionDto) => void,
  addLocal?: () => void,
): MenuItem[] {
  const again = [...new Set(split.panes.map((p) => p.connectionId))]
    .map((id) => connections.find((c) => c.id === id))
    .filter((c): c is ConnectionDto => c !== undefined);
  const ssh = connections
    .filter((c) => c.type === 'ssh' || c.type === 'mikrotik')
    .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: 'base', numeric: true }));
  const item = (c: ConnectionDto): MenuItem => ({ label: c.name, action: () => add(c) });
  const items: MenuItem[] = [];
  if (again.length > 0) items.push({ label: t('Again'), submenu: again.map(item) });
  if (ssh.length > 0) items.push({ label: t('SSH connections'), submenu: ssh.map(item) });
  if (addLocal) items.push({ label: t('Local Terminal'), action: addLocal });
  return items;
}
