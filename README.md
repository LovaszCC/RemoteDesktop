<div align="center">

<img src="app/src-tauri/icons/icon.png" width="128" alt="RemoteDesktop icon">

# RemoteDesktop

### ⚡ One app for **RDP · SSH · Web · MikroTik** on macOS, Windows and Linux

*A fast, lightweight alternative to Devolutions Remote Desktop Manager.*<br>
*Rust + Tauri · instant start · everything behind one master password.*

[![Beta](https://img.shields.io/github/v/release/LovaszCC/RemoteDesktop?include_prereleases&style=for-the-badge&logo=github&color=2563eb&label=beta)](https://github.com/LovaszCC/RemoteDesktop/releases/tag/beta-latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-22c55e?style=for-the-badge)](LICENSE)
<br>
![macOS](https://img.shields.io/badge/macOS-Apple%20Silicon-000000?style=for-the-badge&logo=apple&logoColor=white)
![Windows](https://img.shields.io/badge/Windows-x64-0078D6?style=for-the-badge&logo=windows&logoColor=white)
![Linux](https://img.shields.io/badge/Linux-x86__64-FCC624?style=for-the-badge&logo=linux&logoColor=black)
![Rust](https://img.shields.io/badge/Rust-Tauri%202-CE422B?style=for-the-badge&logo=rust&logoColor=white)

**[⬇️ Download](https://github.com/LovaszCC/RemoteDesktop/releases/tag/beta-latest)** ·
**[✨ Features](#-features)** ·
**[🔐 Security](#-security)** ·
**[🛠 Build](#-build-from-source)**

🇬🇧 English · 🇭🇺 Magyar — switch in *Settings → Language* (applies at once)

</div>

> [!WARNING]
> **Beta.** This is the cross-platform rewrite of the macOS app on `main`. It uses the same identity: on
> macOS it replaces the installed RemoteDesktop and opens the same database and remembered key. Back up
> `~/Library/Application Support/RemoteDesktop/connections.sqlite` before the first start.

---

## 🚀 Install

Download the file for your system from the
[beta-latest](https://github.com/LovaszCC/RemoteDesktop/releases/tag/beta-latest) pre-release. It is
rebuilt on every change to the `beta` branch.

### 🍎 macOS (Apple Silicon)

1. Download `RemoteDesktop_<version>_macos_aarch64.dmg`.
2. Open it and drag **RemoteDesktop** to **Applications**.
3. Allow the first launch. Builds are not notarized, so macOS blocks them once: open the app, then
   *System Settings → Privacy & Security → Open Anyway*. Or from Terminal:

   ```sh
   xattr -dr com.apple.quarantine /Applications/RemoteDesktop.app
   ```

### 🪟 Windows (x64)

1. Download `RemoteDesktop_<version>_windows_x64-setup.exe`. It installs for the current user only (no
   administrator rights) into `%LOCALAPPDATA%\RemoteDesktop`.
2. The installer is not code-signed, so SmartScreen may show *"Windows protected your PC"*: click
   **More info**, then **Run anyway**.
3. WebView2 is downloaded during setup if it is missing (it ships with Windows 11 and current Windows 10).

### 🐧 Linux (x86_64)

Built on Ubuntu 22.04, so it needs glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+, …).

| Format | Install |
|---|---|
| **AppImage** | `chmod +x RemoteDesktop_<version>_linux_amd64.AppImage` and run it (needs FUSE 2, e.g. `sudo apt install libfuse2`, or run it with `--appimage-extract-and-run`) |
| **.deb** (Debian, Ubuntu) | `sudo apt install ./RemoteDesktop_<version>_linux_amd64.deb` |
| **.rpm** (Fedora, openSUSE) | `sudo dnf install ./RemoteDesktop_<version>_linux_x86_64.rpm` |

The app needs WebKitGTK 4.1 (`libwebkit2gtk-4.1`), which the .deb and .rpm pull in. Remembering the master
key needs a Secret Service keyring (GNOME Keyring or KWallet).

> [!NOTE]
> Optional apps it can hand off to: [Windows App](https://apps.apple.com/app/windows-app/id1295203466)
> (macOS), `mstsc` (Windows), [Remmina](https://remmina.org) or `xfreerdp` (Linux), an external terminal
> and [WinBox](https://mikrotik.com/download).

### 🔄 Updates

Beta builds don't update themselves yet: download the newest one from
[beta-latest](https://github.com/LovaszCC/RemoteDesktop/releases/tag/beta-latest) and install it over the
old one. The in-app updater (*Settings → Updates*, *Check for Updates…*) switches on with the first signed
release; every update is verified with the project's minisign key before it runs.

> [!IMPORTANT]
> macOS: after every update, grant the Accessibility permission again (*System Settings → Privacy &
> Security → Accessibility*). It is only needed for the Windows App password autofill.

---

## 🔗 Connection types

| | Type | Opens in | Highlights |
|:-:|---|---|---|
| 🖥️ | **RDP** | Embedded tab (FreeRDP 3.32.2) or the system client: Windows App, `mstsc`, Remmina / `xfreerdp` | Shared clipboard with files both ways, resolution follows the tab, certificate prompts, reconnect, admin session (`/admin`) |
| 💻 | **SSH** | Embedded terminal tab (xterm.js) or an external terminal | System `ssh`, so `~/.ssh/config`, `known_hosts` and jump hosts work; password and key passphrase supplied automatically |
| 🌐 | **Web** | Chosen browser or the default one | Password, then username copied to the clipboard for pasting into the login form |
| 📶 | **MikroTik** | SSH or WinBox | WinBox logs in directly (native, or `winbox64.exe` through Wine on Linux) |

<details open>
<summary><b>🖥️ RDP</b></summary>
<br>

Credentials go straight into FreeRDP, never on a command line. When a server requires NLA and the
connection has no username or password, the embedded client asks for them; *Save to this connection*
stores them (encrypted) for next time, *Cancel* closes the tab. *Type clipboard* types the local clipboard
into the session as keystrokes, which works where clipboard sharing does not (logon screen, consoles). The
clipboard carries text and **files both ways**: files copied on the server are downloaded as soon as the
server announces them (with progress and Cancel in the tab) and land on the local clipboard; files copied
locally are offered to the server. File names from the server are checked so nothing can land outside the
download folder, and downloaded files are marked like browser downloads (macOS quarantine, Windows
Mark-of-the-Web). A logoff closes the tab; a dropped connection keeps it with the reason and a reconnect
button.

With the system client no file holds the password: Windows App gets it typed into its prompt (macOS
Accessibility, clipboard fallback for 30 seconds), `mstsc` reads it from a session credential deleted after
60 seconds, Remmina gets it on the clipboard and `xfreerdp` on its standard input.

</details>

<details open>
<summary><b>💻 SSH</b></summary>
<br>

Uses `SSH_ASKPASS` with the app itself as the helper, so no `sshpass` is needed; unknown host keys are
still confirmed by you, and a changed key is shown with both fingerprints and a *Replace key* button. A
clean `exit` closes the tab, a dropped connection keeps it with a reconnect button. Each connection opens
either embedded or in the external terminal chosen in *Settings → SSH*: Ghostty, iTerm2 or Terminal on
macOS; Windows Terminal or the console on Windows; the system default, GNOME Terminal, Konsole, kitty,
Ghostty or Alacritty on Linux.

**Local terminal**: the fixed *Local Terminal* entry at the top of the sidebar opens this computer's shell
in a tab (login shell on macOS and Linux, PowerShell on Windows).

**Copy next output** (embedded terminals): press it, type a command (e.g. `cat /etc/raddb/radiusd.conf`)
and press Enter. Its output goes to the clipboard as plain text, without colors, the echoed command or the
prompt, and a toast shows how many lines were copied. It runs in the terminal itself, so it works after
`sudo -i` / `su`, on a host you reached with a nested `ssh`, and on RouterOS (`/export`). The recording
ends by itself when the prompt comes back; if the command changes the prompt (e.g. `cd`), use *Stop &
copy*. *Cancel* or Esc stops without copying. Interactive programs and pagers (`less`, `vim`,
`systemctl status`) stop the recording: use `cat` or `--no-pager`. Output over 10 MB is refused. In a split
view each pane has its own button.

</details>

<details open>
<summary><b>📡 Split view with broadcast input</b> (embedded SSH / MikroTik SSH / local terminal)</summary>
<br>

Select 2–4 connections in the sidebar and choose *Open in split view* to open them side by side in one
tab — 2 next to each other, 3 as one left and two stacked right, 4 as a 2×2 grid, with draggable dividers.
*Add terminal* adds a pane, also for a server that is already open (e.g. `tail -f` the syslog in one, type
commands in the other). For one server: *Open in split view (2×)* in its context menu, or *Split terminal*
in an open SSH tab, which keeps the running session and opens the same server next to it.

Each pane has a **broadcast** toggle: what you type or paste into a broadcasting pane also goes to every
other broadcasting pane, iTerm2 style — configure several switches at once. *Broadcast to all* switches it
on or off for all panes. Broadcast is off by default; broadcasting panes have an orange frame and a
*BROADCAST* badge, and the tab turns orange. The pane you type into has a green frame.

**💾 Saved splits**: save a split tab (its context menu → *Save split…*) or 2–4 selected SSH entries (*Save
as split…*) by name. They are listed under *Saved splits* below Favorites: double-click to open (broadcast
always starts off), drag to reorder, rename or delete from the context menu. A tab opened from a saved split
offers *Update saved split* after panes were added or closed.

</details>

<details open>
<summary><b>📁 Files (SCP)</b></summary>
<br>

Right-click an SSH tab (or entry) for a file browser window — the server on the left, this computer on the
right. Drag between them (or from the file manager onto the server side), or select rows and use the arrow
buttons. Folders travel as a single tar stream instead of file by file, gzip-compressed by default (toolbar
toggle), with byte progress, cancel and a replace / skip prompt for existing items. Opened from a tab, it
starts in the shell's current directory. One SSH connection serves the whole window, with the entry's
login. Servers without `tar` (e.g. RouterOS) copy single files over SFTP.

</details>

<details open>
<summary><b>⚠️ Root warning</b></summary>
<br>

While an embedded SSH session is root, its tab (and split pane header) shows a yellow warning sign — when
logging in as `root`, and after `sudo -i` / `su -`, read from the `user@host` title most shells set.

</details>

<details open>
<summary><b>🔀 Port forwards</b> (SSH connections)</summary>
<br>

Save forwards such as `localhost:13389 → server.local:3389` on the connection's page and start or stop
them with one click; a green dot shows a running tunnel, and the sidebar marks connections with one. Each
runs inside the app with the connection's login, listening on `127.0.0.1` only. An unknown server key is
shown with its fingerprint for confirmation first and then saved to `~/.ssh/known_hosts`. Tunnels stop when
you lock or quit the app. `ProxyJump` / `ProxyCommand` are not supported for tunnels and the file browser.

</details>

---

## ✨ Features

| | |
|---|---|
| 🔑 **Linked credentials** | Reusable user / domain / password / SSH key entries that many connections can share. Passphrase-protected SSH keys are unlocked once on save and stored encrypted. A credential without a username (e.g. just an SSH key) can be used with a different username on each connection. Select several connections or folders to link them to a credential in one go. A credential's *Used by* list is searchable and can add connections or unlink the checked ones. |
| 🗂️ **Folders and favourites** | Nested folders with drag & drop (several selected items move together, or use *Move* or *Delete* in their context menu) and an optional icon color, favourites on top. |
| 🔍 **Instant search** | Across name, host, user, notes and folder. |
| 🗃️ **Tabs** | For embedded sessions; quitting with open sessions asks first. A connection's color (e.g. red for production) colors its tabs and a stripe above the session; any tab can be recolored from its context menu and renamed (double-click or *Rename…*). |
| 📡 **Split view** | Up to 4 terminals with iTerm2-style broadcast input. |
| 📋 **Copy next output** | Copies the next command's output in an SSH terminal to the clipboard, even as root after `sudo -i`. |
| 🔀 **SSH port forwards** | Saved per connection, started and stopped with one click. |
| 🌍 **English and Hungarian** | Interface language switchable in Settings, without a restart. |
| 🔄 **Updates** | Signed in-app updates from GitHub releases. |
| 📥 **Import** | From a Devolutions RDM `.rdm` export or a WinBox address book (see below). |
| ☁️ **Portable database** | A single SQLite file whose location you choose in Settings, so it can live in iCloud Drive, OneDrive or Dropbox. It is the same file the macOS app on `main` uses. |

### ⌨️ Keyboard shortcuts

Defaults below; every one can be changed in *Settings → Keyboard Shortcuts* (a key already used by another
command or by the system is refused; on Windows and Linux Ctrl+Alt+character is refused, because it is
AltGr on many layouts).

| Action | macOS | Windows / Linux |
|---|:-:|:-:|
| Connect (several selected SSH connections: split view) | Double-click / <kbd>⌘</kbd><kbd>↩</kbd> | Double-click / <kbd>Ctrl</kbd><kbd>Enter</kbd> |
| New RDP / SSH / web / MikroTik connection | <kbd>⌘N</kbd> / <kbd>⇧⌘N</kbd> / <kbd>⇧⌘B</kbd> / <kbd>⇧⌘M</kbd> | <kbd>Ctrl+Shift+N</kbd> / <kbd>S</kbd> / <kbd>B</kbd> / <kbd>M</kbd> |
| New folder / credential | <kbd>⌥⌘N</kbd> / <kbd>⌃⌘N</kbd> | <kbd>Ctrl+Shift+G</kbd> / <kbd>P</kbd> |
| New local terminal | <kbd>⌘T</kbd> | <kbd>Ctrl+Shift+H</kbd> |
| Import from RDM / WinBox | <kbd>⌘I</kbd> / <kbd>⇧⌘I</kbd> | <kbd>Ctrl+Shift+I</kbd> / <kbd>X</kbd> |
| Lock | <kbd>⌃⌘L</kbd> | <kbd>Ctrl+Shift+L</kbd> |
| Toggle sidebar | <kbd>⌃⌘S</kbd> | <kbd>Ctrl+Shift+E</kbd> |
| Close tab / reconnect | <kbd>⌘W</kbd> / <kbd>⌘R</kbd> | <kbd>Ctrl+Shift+W</kbd> / <kbd>R</kbd> |
| Next / previous tab | <kbd>⌃Tab</kbd> / <kbd>⌃⇧Tab</kbd> | <kbd>Ctrl+Tab</kbd> / <kbd>Ctrl+Shift+Tab</kbd> |
| Type clipboard into RDP session | <kbd>⇧⌘V</kbd> | <kbd>Ctrl+Shift+T</kbd> |
| Split terminal (same server in a new pane) | <kbd>⌘D</kbd> | <kbd>Ctrl+Shift+D</kbd> |
| Broadcast input to all panes of a split view | <kbd>⌥⌘I</kbd> | <kbd>Ctrl+Shift+A</kbd> |
| Clear the active terminal (screen + scrollback, prompt redrawn) | <kbd>⌘K</kbd> | <kbd>Ctrl+Shift+K</kbd> |
| Copy the next command's output in the active terminal (again: stop & copy) | <kbd>⌥⌘C</kbd> | <kbd>Ctrl+Shift+O</kbd> |
| Files (SCP) for the active SSH tab or selected entry | <kbd>⇧⌘F</kbd> | <kbd>Ctrl+Shift+F</kbd> |

**Inside RDP sessions** the keyboard behaves like the local one:

| Key | macOS | Windows / Linux |
|---|---|---|
| <kbd>⌥</kbd> / <kbd>AltGr</kbd> + key | Types the Mac character (`@ \| [ ] { } €` …, e.g. right ⌥Q → `@` on the Hungarian layout) whatever the server's layout; with keys that type nothing (Tab, F4, arrows) it is Alt | AltGr types the local layout's character; Alt stays Alt |
| <kbd>⌘</kbd> + key | Ctrl + key (<kbd>⌘C</kbd> / <kbd>⌘V</kbd> copy and paste in Windows); app shortcuts such as <kbd>⌘W</kbd> stay with the app | — |
| <kbd>⌘</kbd> alone / <kbd>Win</kbd> | Windows key (Start menu) | Windows key; captured while the session has focus (X11 only, not on Wayland) |

---

## 🔐 Security

> [!TIP]
> Passwords and key passphrases are encrypted with **AES-256-GCM**. The key is derived from your master
> password with **PBKDF2-SHA256 (600,000 iterations)**.

- Salt and verifier are stored in the database, so the file can move between computers.
- *Remember on this computer* keeps the derived key in the OS keyring (macOS Keychain, Windows Credential
  Manager, Linux Secret Service).
- Names, hosts and user names are stored unencrypted, for fast listing and search.
- Secrets stay in the Rust core; the user interface never receives a stored password.
- SSH passwords reach `ssh` through the app's own askpass helper over a per-user socket or named pipe;
  unlocked keys are handed over as `0600` files in a `0700` temporary folder and deleted with the session.
- Copied passwords are marked as concealed for clipboard managers and cleared after 30 seconds.

---

## 📥 Importing

### From Devolutions RDM

In RDM choose *File → Export → Export all entries…* and the **.rdm** format. To bring passwords along,
tick *include credentials* and **do not** encrypt the export; otherwise passwords must be entered again by
hand (the import summary tells you how many). Folders, RDP, SSH and credential entries and their links are
imported.

> [!WARNING]
> Delete the export file afterwards.

### From WinBox

*File → Import from WinBox…* reads a WinBox address book (`.cdb`, e.g. an exported `addresses.cdb`). A
preview lists the entries; pick a target folder — WinBox groups become subfolders in it — and the entries
to import (ones already there are unchecked). They become MikroTik connections with their login; a custom
WinBox port is noted in the entry's notes.

> [!WARNING]
> WinBox keeps passwords in plain text, so delete the export afterwards.

---

## 🛠 Build from source

Requires [rustup](https://rustup.rs), **Node 26** and **pnpm 12**, plus CMake, Ninja and Perl for the
bundled FreeRDP:

- macOS: Xcode Command Line Tools, `brew install cmake ninja`;
- Windows: Visual Studio Build Tools with *Desktop development with C++*, Strawberry Perl;
- Linux: `sudo apt-get install -y libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev
  librsvg2-dev build-essential file cmake ninja-build perl curl`.

```sh
pnpm -C app/ui install
pnpm -C app/ui tauri dev        # run the app
cargo test --workspace          # Rust tests
pnpm -C app/ui test             # UI tests
scripts/build-macos.sh          # release bundles (also build-linux.sh, build-windows.ps1)
```

The first build also compiles a static FreeRDP + OpenSSL from pinned sources (`scripts/build-freerdp.sh`,
`scripts/build-freerdp.ps1`; a few minutes, once per target).

---

## 📜 License

MIT, see [LICENSE](LICENSE). Bundled third-party software: FreeRDP and OpenSSL (Apache License 2.0), and
the Rust and npm packages listed in `Cargo.lock` and `app/ui/pnpm-lock.yaml` under their own licenses.

<div align="center">
<br>

**Built with ❤️ for sysadmins who live in terminals.**

<sub>Rust · Tauri · FreeRDP · xterm.js · OpenSSL</sub>

</div>
