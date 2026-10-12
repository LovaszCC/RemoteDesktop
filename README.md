# RemoteDesktop

[![Release](https://img.shields.io/github/v/release/LovaszCC/RemoteDesktop)](https://github.com/LovaszCC/RemoteDesktop/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
![Platform](https://img.shields.io/badge/macOS-15%2B%20·%20Apple%20Silicon-lightgrey)

A lightweight, native macOS connection manager for **RDP, SSH, web and MikroTik** connections: a fast
alternative to Devolutions Remote Desktop Manager. Around 30 MB of memory, instant start, everything
behind one master password.

> Available in **English** and **Hungarian**. Switch in *Settings → Language*; it applies after a restart.

## Install

1. Download `RemoteDesktop-<version>.dmg` from the
   [latest release](https://github.com/LovaszCC/RemoteDesktop/releases/latest).
2. Open it and drag **RemoteDesktop** to **Applications**.
3. Allow the first launch. Builds are not notarized, so macOS blocks them once: open the app, then
   *System Settings → Privacy & Security → Open Anyway*. Or from Terminal:

   ```sh
   xattr -dr com.apple.quarantine /Applications/RemoteDesktop.app
   ```

Requires an Apple Silicon Mac with macOS 15 or later. Optional apps it can hand off to:
[Windows App](https://apps.apple.com/app/windows-app/id1295203466), [Ghostty](https://ghostty.org) and
[WinBox](https://mikrotik.com/download).

> After every update, grant the Accessibility permission again (*System Settings → Privacy & Security →
> Accessibility*). It is only needed for the Windows App password autofill.

## Connection types

| Type | Opens in | Highlights |
|---|---|---|
| **RDP** | Embedded tab (FreeRDP 3.32.1) or Windows App | Shared clipboard, resolution follows the tab, certificate prompts, reconnect, admin session (`/admin`) |
| **SSH** | Embedded terminal tab (SwiftTerm) or Ghostty | System `ssh`, so `~/.ssh/config`, `known_hosts` and jump hosts work; password and key passphrase supplied automatically |
| **Web** | Chosen browser or the default one | Password, then username copied to the clipboard for pasting into the login form |
| **MikroTik** | SSH or WinBox | WinBox logs in directly; double-click uses the per-connection default mode |

**RDP** credentials go straight into FreeRDP, never on a command line. The *Type clipboard* button
(⇧⌘V) types the local clipboard into the session as keystrokes, which works where clipboard sharing does
not (logon screen, consoles). A logoff closes the tab; a dropped connection keeps it with the reason and a
reconnect button. With Windows App, the password is typed into its prompt automatically, or copied to the
clipboard for 30 seconds as a fallback.

**SSH** uses `SSH_ASKPASS`, so no `sshpass` is needed; unknown host keys are still confirmed by you. A
clean `exit` closes the tab, a dropped connection keeps it with a reconnect button.

## Features

- **Linked credentials**: reusable user / domain / password / SSH key entries that many connections can
  share. Passphrase-protected SSH keys are unlocked once on save and stored encrypted.
- **Folders and favourites**: nested folders with drag & drop, favourites on top.
- **Instant search** across name, host, user, notes and folder.
- **Tabs** for embedded sessions; quitting with open sessions asks first.
- **English and Hungarian** interface, switchable in Settings.
- **Import** from a Devolutions RDM `.rdm` export (see below).
- **Portable database**: a single SQLite file whose location you choose in Settings, so it can live in
  iCloud Drive or Dropbox.

### Keyboard shortcuts

| Action | Shortcut |
|---|---|
| Connect | Double-click / ⌘↩ |
| New RDP / SSH / web / MikroTik connection | ⌘N / ⇧⌘N / ⇧⌘B / ⇧⌘M |
| New folder / credential | ⌥⌘N / ⌃⌘N |
| Import | ⌘I |
| Lock | ⌃⌘L |
| Close tab / reconnect | ⌘W / ⌘R |
| Next / previous tab | ⌃Tab / ⌃⇧Tab |
| Type clipboard into RDP session | ⇧⌘V |

## Security

- Passwords and key passphrases are encrypted with **AES-256-GCM**. The key is derived from your master
  password with PBKDF2-SHA256 (600,000 iterations). Salt and verifier are stored in the database, so the
  file can move between Macs.
- *Remember on this Mac* keeps the derived key in the login Keychain (`WhenUnlockedThisDeviceOnly`).
- Names, hosts and user names are stored unencrypted, for fast listing and search.
- SSH secrets are handed over through `0600` files in a `0700` temporary folder and deleted on first use
  (at most 90 seconds).

## Importing from Devolutions RDM

In RDM choose *File → Export → Export all entries…* and the **.rdm** format. To bring passwords along,
tick *include credentials* and **do not** encrypt the export; otherwise passwords must be entered again by
hand (the import summary tells you how many). Folders, RDP, SSH and credential entries and their links are
imported. Delete the export file afterwards.

## Build from source

Requires Xcode 26 or later and `brew install cmake ninja`.

```sh
xcodebuild -downloadComponent MetalToolchain   # once; SwiftTerm compiles a Metal shader
scripts/build-app.sh --install                 # build, sign and copy to /Applications
swift test                                     # unit tests
```

The first build also compiles a static FreeRDP + OpenSSL (`scripts/build-freerdp.sh`, about 10 minutes);
local FreeRDP fixes live in `patches/freerdp/`. Signing uses your first *Apple Development* identity, so
the Accessibility permission survives rebuilds; set `SIGN_IDENTITY=-` for an ad-hoc signature.

## License

MIT, see [LICENSE](LICENSE). Bundled third-party software (FreeRDP, OpenSSL, SwiftTerm, GRDB) and its
licenses are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
