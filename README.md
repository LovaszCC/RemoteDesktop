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
[Windows App](https://apps.apple.com/app/windows-app/id1295203466), [Ghostty](https://ghostty.org),
[iTerm2](https://iterm2.com) and [WinBox](https://mikrotik.com/download).

### Updates

The app checks GitHub for a new release at launch (at most once a day; turn it off in *Settings → Updates*)
and from **RemoteDesktop → Check for Updates…**. **Download** saves the verified disk image to *Downloads*
and opens it: quit RemoteDesktop and drag the new version to **Applications**. Images downloaded this way
skip the first-launch *Open Anyway* step.

> After every update, grant the Accessibility permission again (*System Settings → Privacy & Security →
> Accessibility*). It is only needed for the Windows App password autofill.

## Connection types

| Type | Opens in | Highlights |
|---|---|---|
| **RDP** | Embedded tab (FreeRDP 3.32.1) or Windows App | Shared clipboard, resolution follows the tab, certificate prompts, reconnect, admin session (`/admin`) |
| **SSH** | Embedded terminal tab (SwiftTerm) or an external terminal: Ghostty, iTerm2 or Terminal | System `ssh`, so `~/.ssh/config`, `known_hosts` and jump hosts work; password and key passphrase supplied automatically |
| **Web** | Chosen browser or the default one | Password, then username copied to the clipboard for pasting into the login form |
| **MikroTik** | SSH or WinBox | WinBox logs in directly; double-click uses the per-connection default mode |

**RDP** credentials go straight into FreeRDP, never on a command line. The *Type clipboard* button
(⇧⌘V) types the local clipboard into the session as keystrokes, which works where clipboard sharing does
not (logon screen, consoles). A logoff closes the tab; a dropped connection keeps it with the reason and a
reconnect button. With Windows App, the password is typed into its prompt automatically, or copied to the
clipboard for 30 seconds as a fallback.

**SSH** uses `SSH_ASKPASS`, so no `sshpass` is needed; unknown host keys are still confirmed by you. A
clean `exit` closes the tab, a dropped connection keeps it with a reconnect button. Each connection opens
either embedded or in the external terminal chosen in *Settings → SSH* (Ghostty, iTerm2 or Terminal;
Ghostty by default when installed, otherwise Terminal). iTerm2 is driven via AppleScript, so macOS asks
once for the *Automation* permission.

**Split view with broadcast input** (embedded SSH / MikroTik SSH): select 2–4 connections in the sidebar
(⌘-click) and choose *Open in split view* (or ⌘↩) to open them side by side in one tab — 2 next to each
other, 3 as one left and two stacked right, 4 as a 2×2 grid, with draggable dividers. *Add terminal* adds a
pane, also for a server that is already open (e.g. `tail -f` the syslog in one, type commands in the other).
For one server: *Open in split view (2×)* in its context menu, or ⌘D / the split button in an open SSH tab,
which keeps the running session and opens the same server next to it.
Each pane has a **broadcast** toggle: what you type or paste into a broadcasting pane also goes to every other
broadcasting pane, iTerm2 style — configure several switches at once. ⌥⌘I switches broadcast on or off for
all panes. Broadcast is off by default; broadcasting panes have an orange frame and a *BROADCAST* badge, and
the tab turns orange. The pane you type into has a green frame.

**Files (SCP)**: right-click an SSH tab (or entry, or ⇧⌘F) for a file browser window — the server on the
left, this Mac on the right. Drag between them (or from Finder onto the server side), or select rows and use
the arrow buttons. Folders travel as a single tar stream instead of file by file, gzip-compressed by default
(toolbar toggle), with byte progress, cancel and a replace / skip prompt for existing items. Opened from a tab,
it starts in the shell's current directory. One SSH connection serves the whole window, with the entry's
login. Servers without `tar` (e.g. RouterOS) copy single files with `scp`.

**Root warning**: while an embedded SSH session is root, its tab (and split pane header) shows a yellow
warning sign — when logging in as `root`, and after `sudo -i` / `su -`, read from the `user@host` title most
shells set.

**Port forwards** (SSH connections): save forwards such as `localhost:13389 → 10.0.0.20:3389` on the
connection's page and start or stop them with one click; a green dot shows a running tunnel, and the
sidebar marks connections with one. Each runs `ssh -N -L` in the background with the connection's login,
listening on `127.0.0.1` only. An unknown server key is shown with its fingerprint for confirmation first
and then saved to `~/.ssh/known_hosts`. Tunnels stop when you quit the app.

## Features

- **Linked credentials**: reusable user / domain / password / SSH key entries that many connections can
  share. Passphrase-protected SSH keys are unlocked once on save and stored encrypted. A credential without a
  username (e.g. just an SSH key) can be used with a different username on each connection.
- **Folders and favourites**: nested folders with drag & drop (several selected items move together, or
  use *Move* or *Delete* in their context menu) and an optional icon color, favourites on top.
- **Instant search** across name, host, user, notes and folder.
- **Tabs** for embedded sessions; quitting with open sessions asks first. A connection's color (e.g. red
  for production) colors its tabs and a stripe above the session; any tab can be recolored from its context menu
  and renamed (double-click or *Rename…*).
- **Split view** of up to 4 SSH terminals with iTerm2-style broadcast input.
- **SSH port forwards** saved per connection, started and stopped with one click.
- **English and Hungarian** interface, switchable in Settings.
- **Update check** against GitHub releases, with a checksum-verified download.
- **Import** from a Devolutions RDM `.rdm` export (see below).
- **Portable database**: a single SQLite file whose location you choose in Settings, so it can live in
  iCloud Drive or Dropbox.

### Keyboard shortcuts

Defaults below; every one can be changed in *Settings → Keyboard Shortcuts* (a key already used by another
command or by macOS is refused).

| Action | Shortcut |
|---|---|
| Connect (several selected SSH connections: split view) | Double-click / ⌘↩ |
| New RDP / SSH / web / MikroTik connection | ⌘N / ⇧⌘N / ⇧⌘B / ⇧⌘M |
| New folder / credential | ⌥⌘N / ⌃⌘N |
| Import from RDM / WinBox | ⌘I / ⇧⌘I |
| Lock | ⌃⌘L |
| Close tab / reconnect | ⌘W / ⌘R |
| Next / previous tab | ⌃Tab / ⌃⇧Tab |
| Type clipboard into RDP session | ⇧⌘V |
| Split terminal (same server in a new pane) | ⌘D |
| Broadcast input to all panes of a split view | ⌥⌘I |
| Files (SCP) for the active SSH tab or selected entry | ⇧⌘F |

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

## Importing from WinBox

*File → Import from WinBox…* (⇧⌘I) reads a WinBox address book (`.cdb`, e.g. an exported `addresses.cdb`).
A preview lists the entries; pick a target folder — WinBox groups become subfolders in it — and the entries
to import (ones already there are unchecked). They become MikroTik connections with their login; a custom
WinBox port is noted in the entry's notes. WinBox keeps passwords in plain text, so delete the export
afterwards.

## Build from source

Requires Xcode 26 or later and `brew install cmake ninja`.

```sh
xcodebuild -downloadComponent MetalToolchain   # once; SwiftTerm compiles a Metal shader
scripts/build-app.sh --install                 # build, sign and copy to /Applications
swift test                                     # unit tests
```

File transfer integration tests run against real SSH servers when `RD_SSH_IT_KEY` (private key) and
`RD_SSH_IT_KNOWN_HOSTS` are set; they expect root logins on `127.0.0.1:2222` (GNU userland) and `:2223`
(busybox) with a `/data` tree — see `Tests/RemoteDesktopCoreTests/RemoteFilesIntegrationTests.swift`.

The first build also compiles a static FreeRDP + OpenSSL (`scripts/build-freerdp.sh`, about 10 minutes);
local FreeRDP fixes live in `patches/freerdp/`. Signing uses your first *Apple Development* identity, so
the Accessibility permission survives rebuilds; set `SIGN_IDENTITY=-` for an ad-hoc signature.

## License

MIT, see [LICENSE](LICENSE). Bundled third-party software (FreeRDP, OpenSSL, SwiftTerm, GRDB) and its
licenses are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
