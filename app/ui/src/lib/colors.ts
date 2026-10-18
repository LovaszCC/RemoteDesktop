// Label colours (Swift `LabelColor`). The values are the `--label-*` tokens in `app.css`, close to the
// macOS system colours and redefined for dark mode.

import { LABEL_COLORS, type LabelColor } from './types';

/** The CSS value of a label colour. */
export function labelColorVar(color: LabelColor): string {
  return `var(--label-${color})`;
}

/** Every label colour with its CSS value, in picker order. */
export const LABEL_COLOR_MAP: Readonly<Record<LabelColor, string>> = Object.freeze(
  Object.fromEntries(LABEL_COLORS.map((c) => [c, labelColorVar(c)])) as Record<LabelColor, string>,
);

/** The picker rule: clicking the selected colour again clears it. */
export function nextColor(current: LabelColor | null, clicked: LabelColor): LabelColor | null {
  return current === clicked ? null : clicked;
}
