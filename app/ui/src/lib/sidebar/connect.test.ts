import { describe, expect, it } from 'vitest';
import { connectRoute } from './connect';

const base = { mikrotikMode: 'winbox', sshTerminal: 'embedded', rdpClient: 'embedded' } as const;

describe('connectRoute', () => {
  it('routes SSH to the embedded session or the external terminal', () => {
    expect(connectRoute({ ...base, type: 'ssh' })).toBe('embedded');
    expect(connectRoute({ ...base, type: 'ssh', sshTerminal: 'external' })).toBe('external');
    expect(connectRoute({ ...base, type: 'ssh' }, { terminal: 'external' })).toBe('external');
  });

  it('routes MikroTik by mode, WinBox by default', () => {
    expect(connectRoute({ ...base, type: 'mikrotik' })).toBe('winbox');
    expect(connectRoute({ ...base, type: 'mikrotik' }, { mode: 'ssh' })).toBe('embedded');
    expect(connectRoute({ ...base, type: 'mikrotik', mikrotikMode: 'ssh', sshTerminal: 'external' })).toBe(
      'external',
    );
    expect(connectRoute({ ...base, type: 'mikrotik', mikrotikMode: 'ssh' }, { mode: 'winbox' })).toBe(
      'winbox',
    );
  });

  it('routes web to the browser and RDP by client', () => {
    expect(connectRoute({ ...base, type: 'web' })).toBe('web');
    expect(connectRoute({ ...base, type: 'rdp' })).toBe('rdpEmbedded');
    expect(connectRoute({ ...base, type: 'rdp', rdpClient: 'windowsApp' }, { rdpClient: 'embedded' })).toBe(
      'rdpEmbedded',
    );
    expect(connectRoute({ ...base, type: 'rdp', rdpClient: 'windowsApp' })).toBe('rdpExternal');
    expect(connectRoute({ ...base, type: 'rdp' }, { rdpClient: 'windowsApp' })).toBe('rdpExternal');
  });
});
