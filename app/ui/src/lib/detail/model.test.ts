import { describe, expect, it } from 'vitest';
import { connection, sidebarLibrary } from '../testing/fixtures';
import type { ConnectionDto, PortForwardDto } from '../types';
import {
  allPicked,
  canCopyPassword,
  connectionFields,
  connectionSubtitle,
  credentialFields,
  directConnectionCount,
  displayValue,
  forwardDestination,
  forwardName,
  pickedAmong,
  setAllPicked,
  setPicked,
  usersOf,
} from './model';

const conn = (overrides: Partial<ConnectionDto>): ConnectionDto => ({ ...connection('c1'), ...overrides });

describe('connectionSubtitle', () => {
  it('describes each type', () => {
    expect(connectionSubtitle(conn({ type: 'rdp' }), 'macos')).toBe('RDP · embedded (FreeRDP)');
    expect(
      connectionSubtitle(conn({ type: 'rdp', rdpClient: 'windowsApp', rdpAdminSession: true }), 'macos'),
    ).toBe('RDP · Windows App · /admin');
    expect(connectionSubtitle(conn({ type: 'rdp', rdpClient: 'windowsApp' }), 'windows')).toBe(
      'RDP · System RDP client',
    );
    expect(connectionSubtitle(conn({ type: 'ssh' }), 'macos')).toBe('SSH · embedded terminal');
    expect(connectionSubtitle(conn({ type: 'ssh', sshTerminal: 'external' }), 'macos')).toBe(
      'SSH · Terminal',
    );
    expect(connectionSubtitle(conn({ type: 'mikrotik', mikrotikMode: 'ssh' }), 'linux')).toBe(
      'MikroTik · default SSH',
    );
    expect(connectionSubtitle(conn({ type: 'web' }), 'macos')).toBe('Web · default browser');
    const brave = conn({ type: 'web', browserBundleId: 'com.brave.Browser' });
    expect(connectionSubtitle(brave, 'macos')).toBe('Web · com.brave.Browser');
    expect(connectionSubtitle(brave, 'macos', () => 'Brave Browser')).toBe('Web · Brave Browser');
  });
});

describe('connectionFields', () => {
  const values = (fields: { label: string; value: string }[]) => fields.map((f) => [f.label, f.value]);

  it('shows the resolved login with "—" for empty values', () => {
    const { before, after } = connectionFields(
      conn({
        type: 'ssh',
        host: 'box',
        effectivePort: 22,
        username: 'own',
        resolved: { username: 'cred', domain: '', hasPassword: true, sshKeyPath: null, hasPassphrase: false },
      }),
    );
    expect(values(before)).toEqual([
      ['Host', 'box'],
      ['Port', '22'],
    ]);
    expect(before[0]!.copy).toBe('box');
    expect(values(after)).toEqual([
      ['User', 'cred'],
      ['Password', '••••••••'],
      ['SSH key', '—'],
      ['Folder', '—'],
    ]);
  });

  it('adds the RDP rows and copies nothing empty', () => {
    const { before, after } = connectionFields(conn({ type: 'rdp', rdpAdminSession: false }));
    expect(values(before)).toContainEqual(['Admin session', 'No']);
    expect(values(after)).toEqual([
      ['User', '—'],
      ['Domain', '—'],
      ['Password', '—'],
      ['Folder', '—'],
    ]);
    expect(after[0]!.copy).toBeUndefined();
  });

  it('shows the normalised URL for web and the key passphrase when stored', () => {
    expect(
      values(connectionFields(conn({ type: 'web', host: 'x.test', webUrl: 'https://x.test' })).before),
    ).toEqual([['URL', 'https://x.test']]);
    const ssh = conn({
      type: 'mikrotik',
      resolved: { username: '', domain: '', hasPassword: false, sshKeyPath: '~/k', hasPassphrase: true },
    });
    expect(values(connectionFields(ssh).after)).toContainEqual(['Key passphrase', '••••••••']);
  });

  it('offers Copy password only for a resolved password', () => {
    expect(canCopyPassword(conn({ hasPassword: true }))).toBe(false);
    expect(
      canCopyPassword(
        conn({
          resolved: { username: '', domain: '', hasPassword: true, sshKeyPath: null, hasPassphrase: false },
        }),
      ),
    ).toBe(true);
  });
});

describe('credential and folder details', () => {
  it('lists the credential fields', () => {
    const credential = { ...sidebarLibrary().credentials[0]!, hasPassphrase: true, hasUnlockedKey: true };
    expect(credentialFields(credential).map((f) => [f.label, f.value])).toEqual([
      ['User', 'admin'],
      ['Domain', '—'],
      ['Password', '••••••••'],
      ['SSH key', '—'],
      ['Key passphrase', '••••••••'],
      ['Unlocked key', 'stored'],
    ]);
  });

  it('counts users and direct children', () => {
    const library = sidebarLibrary();
    expect(usersOf(library, 'k1').map((c) => c.id)).toEqual(['c3']);
    expect(directConnectionCount(library, 'f1')).toBe(1);
    expect(displayValue('')).toBe('—');
  });

  it('formats port forwards', () => {
    const forward: PortForwardDto = {
      id: 'p',
      connectionId: 'c1',
      name: ' ',
      localPort: 8080,
      remoteHost: 'fe80::1',
      remotePort: 80,
      createdAt: '',
    };
    expect(forwardDestination(forward)).toBe('[fe80::1]:80');
    expect(forwardName(forward)).toBe('[fe80::1]:80');
    expect(forwardName({ ...forward, name: 'Web' })).toBe('Web');
  });
});

describe('checkbox sets', () => {
  it('select all acts on the visible rows only', () => {
    let picked: ReadonlySet<string> = new Set(['hidden']);
    expect(allPicked(picked, ['a', 'b'])).toBe(false);
    picked = setAllPicked(picked, ['a', 'b'], true);
    expect([...picked].sort()).toEqual(['a', 'b', 'hidden']);
    expect(allPicked(picked, ['a', 'b'])).toBe(true);
    picked = setAllPicked(picked, ['a', 'b'], false);
    expect([...picked]).toEqual(['hidden']);
    expect(allPicked(picked, [])).toBe(false);
  });

  it('keeps picks across searches and unlinks only current users', () => {
    let picked: ReadonlySet<string> = new Set();
    picked = setPicked(picked, 'a', true);
    picked = setPicked(picked, 'b', true);
    picked = setPicked(picked, 'b', false);
    expect([...picked]).toEqual(['a']);
    expect(pickedAmong(new Set(['a', 'gone']), ['a', 'c'])).toEqual(['a']);
  });
});
