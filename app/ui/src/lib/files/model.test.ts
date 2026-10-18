import { describe, expect, it } from 'vitest';
import type { FileEntryDto, FilesDoneDto, FilesProgressDto, FilesStatusDto } from '../types';
import {
  baseName,
  clickSelection,
  compareNames,
  connectFailure,
  dirName,
  doneErrors,
  doneSummary,
  formatDate,
  formatSize,
  groupByFolder,
  isOpenable,
  joinLocal,
  joinRemote,
  nextSort,
  progressBytes,
  progressFraction,
  progressTitle,
  sortEntries,
} from './model';

function entry(name: string, kind: FileEntryDto['kind'] = 'file', size = 0, modified: number | null = null) {
  return { name, kind, size, modified, permissions: kind === 'directory' ? 'drwxr-xr-x' : '-rw-r--r--' };
}

const progress = (overrides: Partial<FilesProgressDto> = {}): FilesProgressDto => ({
  browserId: 'files-1',
  transferId: 't',
  direction: 'download',
  index: 0,
  count: 1,
  name: 'a.txt',
  bytesDone: 500,
  bytesTotal: 1000,
  ...overrides,
});

describe('files model', () => {
  it('sorts folders first, Finder style, by any column', () => {
    const rows = [
      entry('file10', 'file', 5, 30),
      entry('b', 'directory'),
      entry('File2', 'file', 9, 10),
      entry('Álom', 'file', 1, 20),
    ];
    expect(sortEntries(rows, { key: 'name', ascending: true }).map((e) => e.name)).toEqual([
      'b',
      'Álom',
      'File2',
      'file10',
    ]);
    expect(sortEntries(rows, { key: 'name', ascending: false }).map((e) => e.name)).toEqual([
      'b',
      'file10',
      'File2',
      'Álom',
    ]);
    expect(sortEntries(rows, { key: 'size', ascending: true }).map((e) => e.name)).toEqual([
      'b',
      'Álom',
      'file10',
      'File2',
    ]);
    expect(sortEntries(rows, { key: 'modified', ascending: false }).map((e) => e.name)[1]).toBe('file10');
    expect(sortEntries(rows, { key: 'permissions', ascending: true })[0]?.name).toBe('b');
    expect(compareNames('a', 'a')).toBe(0);
    expect(nextSort({ key: 'name', ascending: true }, 'name')).toEqual({ key: 'name', ascending: false });
    expect(nextSort({ key: 'name', ascending: false }, 'size')).toEqual({ key: 'size', ascending: true });
    expect(isOpenable(entry('l', 'symlink'))).toBe(true);
    expect(isOpenable(entry('f'))).toBe(false);
  });

  it('formats sizes and dates', () => {
    expect(formatSize(999, 'en')).toBe('999 B');
    expect(formatSize(1500, 'en')).toBe('1.5 KB');
    expect(formatSize(25_000_000, 'en')).toBe('25 MB');
    expect(formatSize(1_500_000_000, 'hu')).toBe('1,5 GB');
    expect(formatDate(null, 'en')).toBe('');
    expect(formatDate(1_760_000_000, 'en')).toMatch(/2025/);
  });

  it('joins and splits local and remote paths', () => {
    expect(joinRemote('/', 'etc')).toBe('/etc');
    expect(joinRemote('/a', 'b c')).toBe('/a/b c');
    expect(joinLocal('/Users/me', 'x')).toBe('/Users/me/x');
    expect(joinLocal('C:\\Users\\me', 'x')).toBe('C:\\Users\\me\\x');
    expect(joinLocal('C:\\', 'x')).toBe('C:\\x');
    expect(baseName('/a/b/')).toBe('b');
    expect(baseName('C:\\a\\b.txt')).toBe('b.txt');
    expect(dirName('/a/b')).toBe('/a');
    expect(dirName('/a')).toBe('/');
    expect(dirName('C:\\a')).toBe('C:\\');
    expect(dirName('C:\\a\\b')).toBe('C:\\a');
    expect(dirName('x')).toBe('x');
    expect(groupByFolder(['/a/1', '/b/2', '/a/3'])).toEqual([
      { folder: '/a', paths: ['/a/1', '/a/3'] },
      { folder: '/b', paths: ['/b/2'] },
    ]);
  });

  it('selects with click, toggle and range', () => {
    const rows = ['a', 'b', 'c', 'd'];
    const plain = { toggle: false, range: false };
    expect(clickSelection([], rows, 'b', null, plain)).toEqual({ selection: ['b'], anchor: 'b' });
    expect(clickSelection(['b'], rows, 'd', 'b', { toggle: false, range: true })).toEqual({
      selection: ['b', 'c', 'd'],
      anchor: 'b',
    });
    expect(clickSelection(['b'], rows, 'a', 'b', { toggle: false, range: true }).selection).toEqual([
      'a',
      'b',
    ]);
    expect(clickSelection(['b'], rows, 'c', 'b', { toggle: true, range: false }).selection).toEqual([
      'b',
      'c',
    ]);
    expect(clickSelection(['b', 'c'], rows, 'b', 'c', { toggle: true, range: false }).selection).toEqual([
      'c',
    ]);
    expect(clickSelection(['b'], rows, 'c', 'gone', { toggle: false, range: true }).selection).toEqual(['c']);
  });

  it('describes progress and results', () => {
    expect(progressFraction(progress())).toBe(0.5);
    expect(progressFraction(progress({ bytesDone: 2000 }))).toBe(0.99);
    expect(progressFraction(progress({ bytesTotal: null }))).toBeNull();
    expect(progressTitle(progress())).toBe('Downloading “a.txt”');
    expect(progressTitle(progress({ direction: 'upload', index: 1, count: 3 }))).toBe(
      'Uploading “a.txt” (2 of 3)',
    );
    expect(progressBytes(progress(), 'en')).toBe('500 B of 1 KB');
    expect(progressBytes(progress({ bytesTotal: null }), 'en')).toBe('500 B');

    const done = (items: FilesDoneDto['items'], cancelled = false): FilesDoneDto => ({
      browserId: 'files-1',
      transferId: 't',
      direction: 'upload',
      items,
      cancelled,
    });
    expect(doneSummary(done([], true))).toBe('Cancelled');
    expect(doneSummary(done([]))).toBe('Done');
    const mixed = done([
      { name: 'a', status: 'done', error: null },
      { name: 'b', status: 'skipped', error: null },
      { name: 'c', status: 'failed', error: 'Folders need tar on the server.' },
      { name: 'd', status: 'failed', error: 'tar: d: Permission denied' },
    ]);
    expect(doneSummary(mixed)).toBe('1 copied, 1 skipped, 2 failed');
    expect(doneErrors(mixed)).toEqual(['c: Folders need tar on the server.', 'd: tar: d: Permission denied']);
  });

  it('translates connection failures by code', () => {
    const failed = (code: string, error = 'English'): FilesStatusDto => ({
      browserId: 'files-1',
      connectionId: '1',
      connectionName: 'x',
      startPath: '~',
      transferring: false,
      state: 'failed',
      code,
      error,
    });
    expect(connectFailure(failed('authFailed'))).toBe('Authentication failed');
    expect(connectFailure(failed('proxyUnsupported'))).toBe(
      'ProxyJump and ProxyCommand are not supported for the file browser.',
    );
    expect(connectFailure(failed('other', 'Raw text'))).toBe('Raw text');
    expect(connectFailure({ ...failed('x'), state: 'connecting' } as unknown as FilesStatusDto)).toBe('');
  });
});
