import { compile } from 'svelte/compiler';
import { describe, expect, it } from 'vitest';
import source from './AppShell.svelte?raw';

/** The declarations of the first compiled rule whose selector starts with `selector`. */
function rule(selector: string): string {
  const { css } = compile(source, { filename: 'AppShell.svelte', css: 'external' });
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`${escaped}[.\\w-]*\\s*\\{([^}]*)\\}`).exec(css?.code ?? '')?.[1] ?? '';
}

describe('AppShell', () => {
  // jsdom has no layout (and tests get no component CSS), so this checks the rule itself: without it a
  // split's two stacked terminals grow the main column past the window and the 2×2 / 1 + 2 layouts lose
  // their bottom panes.
  it('lets the main column shrink to the window height', () => {
    expect(rule('.main')).toMatch(/min-height:\s*0[;\s]/);
  });
});

describe('AppShell sidebar', () => {
  it('animates hiding and showing the sidebar, except while dragging', () => {
    expect(rule('.shell')).toMatch(/transition:\s*grid-template-columns/);
    expect(rule('.dragging')).toMatch(/transition:\s*none/);
  });
});
