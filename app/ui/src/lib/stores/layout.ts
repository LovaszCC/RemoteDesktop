// Per-viewer layout preferences kept in localStorage (best effort: storage may be unavailable).

export const SIDEBAR_DEFAULT = 260;
export const SIDEBAR_MIN = 200;
export const SIDEBAR_MAX = 480;
const SIDEBAR_KEY = 'rd.sidebarWidth';

export function clampSidebarWidth(width: number): number {
  if (!Number.isFinite(width)) return SIDEBAR_DEFAULT;
  return Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Math.round(width)));
}

export function loadSidebarWidth(
  storage: Pick<Storage, 'getItem'> | undefined = globalThis.localStorage,
): number {
  try {
    const raw = storage?.getItem(SIDEBAR_KEY);
    return raw ? clampSidebarWidth(Number(raw)) : SIDEBAR_DEFAULT;
  } catch {
    return SIDEBAR_DEFAULT;
  }
}

export function saveSidebarWidth(
  width: number,
  storage: Pick<Storage, 'setItem'> | undefined = globalThis.localStorage,
) {
  try {
    storage?.setItem(SIDEBAR_KEY, String(clampSidebarWidth(width)));
  } catch {
    // Storage unavailable: the width just isn't remembered.
  }
}

const COLLAPSED_KEY = 'rd.sidebarCollapsed';
/** Dragging the sidebar edge this far below the minimum width hides the sidebar. */
export const SIDEBAR_COLLAPSE_AT = SIDEBAR_MIN - 60;

export function loadSidebarCollapsed(
  storage: Pick<Storage, 'getItem'> | undefined = globalThis.localStorage,
): boolean {
  try {
    return storage?.getItem(COLLAPSED_KEY) === '1';
  } catch {
    return false;
  }
}

export function saveSidebarCollapsed(
  collapsed: boolean,
  storage: Pick<Storage, 'setItem'> | undefined = globalThis.localStorage,
) {
  try {
    storage?.setItem(COLLAPSED_KEY, collapsed ? '1' : '0');
  } catch {
    // Storage unavailable: the state just isn't remembered.
  }
}
