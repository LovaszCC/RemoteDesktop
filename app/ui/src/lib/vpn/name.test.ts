import { describe, expect, it } from 'vitest';
import { vpnAppName, vpnText } from './name';

describe('VPN app name', () => {
  it('is MacVPN, WinVPN or LinVPN by platform', () => {
    expect(vpnAppName('macos')).toBe('MacVPN');
    expect(vpnAppName('windows')).toBe('WinVPN');
    expect(vpnAppName('linux')).toBe('LinVPN');
  });

  it('swaps the name in a translated text', () => {
    const text = "MacVPN doesn't let RemoteDesktop use this VPN. Revoke it in MacVPN.";
    expect(vpnText(text, 'macos')).toBe(text);
    expect(vpnText(text, 'windows')).toBe(
      "WinVPN doesn't let RemoteDesktop use this VPN. Revoke it in WinVPN.",
    );
    expect(vpnText('A MacVPN token', 'linux')).toBe('A LinVPN token');
    // The app store says macOS until Rust reports the platform.
    expect(vpnText('MacVPN')).toBe('MacVPN');
  });
});
