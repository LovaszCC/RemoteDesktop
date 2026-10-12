# RemoteDesktop

Lightweight native macOS launcher for saved RDP and SSH connections — a fast replacement for
Devolutions Remote Desktop Manager. ~30 MB memory footprint, instant start.

- **RDP** → opens in **Windows App**; the password is typed into its credential prompt automatically
  (Accessibility API), or copied to the clipboard for 30 s as a fallback.
- **SSH** → opens a new **Ghostty** window running `ssh`; password / key passphrase are supplied
  automatically through `SSH_ASKPASS` (no `sshpass` needed). Unknown host keys are still confirmed by you.

## Features

- **RDP admin session** toggle per connection (`mstsc /admin` equivalent, `administrative session:i:1`)
- **Linked credentials**: reusable user/domain/password/SSH key entries (sidebar *Credentialok*) that
  connections link to; a linked credential replaces the entry's own login. Deleting one asks for
  confirmation and linked entries fall back to their own saved login.
- Nested folders (drag & drop or *Move* menu), favourites, instant search (name, host, user, notes, folder)
- Double-click / ⌘↩ to connect, ⌘N new RDP, ⇧⌘N new SSH, ⌥⌘N new folder, ⌃⌘N new credential, ⌘I import, ⌃⌘L lock
- Import from Devolutions RDM `.rdm` export (folders, RDP, SSH, credential entries and their links;
  plain-text passwords if exported)
- SQLite database, location selectable in Settings (can live in iCloud Drive / Dropbox)

## Security model

- Passwords and key passphrases are encrypted with AES-256-GCM. The key is derived from your master
  password with PBKDF2-SHA256 (600 000 iterations); salt + verifier are stored in the database, so the
  file can be moved between Macs.
- *Remember on this Mac* caches the derived key in the login Keychain (`WhenUnlockedThisDeviceOnly`).
- Names, hosts and user names are stored unencrypted (needed for fast listing/search).
- SSH secrets are written to a `0700` temp directory as `0600` files, deleted on first use (≤ 90 s).

## Build & install

Requires Xcode 16+ / Swift 6 and macOS 15+.

```sh
scripts/build-app.sh --install   # release build, codesign, copy to /Applications
swift test                       # unit tests
swift scripts/make-icon.swift    # regenerate Resources/AppIcon.icns
```

Signing uses the first *Apple Development* identity (override with `SIGN_IDENTITY`), so the
Accessibility permission survives rebuilds. Grant it in *System Settings → Privacy & Security →
Accessibility* (Settings window has a shortcut).

## Importing from Devolutions RDM

RDM → *File → Export → Export all entries…* → **.rdm** format. To import passwords, tick *include
credentials* and **do not** encrypt the export (otherwise passwords stay in RDM's `SafePassword` form and
must be entered manually — the import summary tells you how many). Delete the export file afterwards.
