// Whether the sidebar is shown (the Toggle sidebar command, the toolbar button, dragging its edge);
// remembered per viewer.

import { loadSidebarCollapsed, saveSidebarCollapsed } from './layout';

export function createSidebarLayout(storage?: Pick<Storage, 'getItem' | 'setItem'>) {
  let collapsed = $state(loadSidebarCollapsed(storage));
  const set = (value: boolean) => {
    collapsed = value;
    saveSidebarCollapsed(value, storage);
  };
  return {
    get collapsed() {
      return collapsed;
    },
    set,
    toggle: () => set(!collapsed),
  };
}

export type SidebarLayoutStore = ReturnType<typeof createSidebarLayout>;

export const sidebarLayout = createSidebarLayout();
