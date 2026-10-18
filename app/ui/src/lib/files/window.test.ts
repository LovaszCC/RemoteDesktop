import { describe, expect, it } from 'vitest';
import { FILES_LABEL_PREFIX, windowLabel } from './window';

describe('window label', () => {
  it('reads the Tauri metadata, else main', () => {
    expect(windowLabel(undefined)).toBe('main');
    expect(windowLabel({ metadata: { currentWindow: { label: 'files-1' } } })).toBe('files-1');
    expect(
      windowLabel({ metadata: { currentWebview: { label: 'files-2' }, currentWindow: { label: 'x' } } }),
    ).toBe('files-2');
    expect('files-1'.startsWith(FILES_LABEL_PREFIX)).toBe(true);
  });
});
