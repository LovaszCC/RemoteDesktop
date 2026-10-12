# RemoteDesktop

Lightweight native macOS launcher for saved RDP, SSH, browser and MikroTik connections — a fast replacement for
Devolutions Remote Desktop Manager. ~30 MB memory footprint, instant start.

- **RDP** → opens in an **embedded FreeRDP tab** (statically linked FreeRDP 3.32.1, native Mac client):
  credentials go straight into FreeRDP (never on a command line), clipboard text is shared and the remote
  desktop follows the tab size (Display Control). Unknown or changed server certificates ask first.
  A logoff closes the tab; a dropped connection keeps it with the reason and a reconnect button.
  Per connection, **Windows App** can be used instead: the password is typed into its credential
  prompt automatically (Accessibility API), or copied to the clipboard for 30 s as a fallback.
- **SSH** → opens in an **embedded terminal tab** (SwiftTerm, like RDM) or, per connection, a new
  **Ghostty** window. Both run the system `ssh`, so `~/.ssh/config`, `known_hosts` and jump hosts work;
  password / key passphrase are supplied automatically through `SSH_ASKPASS` (no `sshpass` needed).
  Unknown host keys are still confirmed by you. Tabs: ⌘W closes, ⌃Tab / ⌃⇧Tab switches; a clean `exit`
  closes the tab, a dropped connection keeps it with a reconnect button (⌘R). Quitting with running
  sessions asks first.
- **Browser** → opens the URL in the browser chosen per connection (or the default one), then copies
  the password and, a second later, the username to the clipboard — without the concealed marker or
  auto-clear, so clipboard managers such as Raycast keep both in their history.
- **MikroTik** → SSH (same as SSH entries) or **WinBox** (`WinBox <host> <user> <password>`, logs in
  directly; uses the default WinBox port 8291). Double-click uses the per-connection default mode; the
  detail view and context menu offer both.

## Features

- **RDP admin session** toggle per connection (`mstsc /admin` equivalent, `administrative session:i:1`)
- **Linked credentials**: reusable user/domain/password/SSH key entries (sidebar *Credentialok*) that
  connections link to; a linked credential replaces the entry's own login. Deleting one asks for
  confirmation and linked entries fall back to their own saved login. A passphrase-protected SSH key
  is unlocked on save (`ssh-keygen -p` on a private copy; a wrong passphrase blocks saving) and the
  unlocked key is stored encrypted, so SSH uses it directly without a passphrase prompt or the original file.
- Nested folders (drag & drop or *Move* menu), favourites, instant search (name, host, user, notes, folder)
- Double-click / ⌘↩ to connect, ⌘N new RDP, ⇧⌘N new SSH, ⇧⌘B new browser, ⇧⌘M new MikroTik, ⌥⌘N new folder, ⌃⌘N new credential, ⌘I import, ⌃⌘L lock
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

Requires Xcode 16+ / Swift 6 and macOS 15+. The embedded RDP client needs a one-time static FreeRDP
build (`scripts/build-freerdp.sh`, ~10 min, requires `brew install cmake ninja`); `build-app.sh` runs it
automatically when `Vendor/FreeRDP.xcframework` is missing. Upstream fixes live in `patches/freerdp/`. SwiftTerm compiles a Metal shader, so newer Xcode
versions need the Metal Toolchain once: `xcodebuild -downloadComponent MetalToolchain`.

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
