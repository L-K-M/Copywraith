# Copywraith on KDE / Linux

The desktop app is built from the same Tauri + Svelte codebase as the macOS
build. This page covers the KDE Plasma (Wayland-first) integration: system tray,
paste injection, global shortcuts, and autostart.

> [!NOTE]
> Plasma 6 defaults to a **Wayland** session, which forbids apps from injecting
> keystrokes directly. Copywraith uses [`ydotool`](https://github.com/ReimuNotMoe/ydotool)
> (a uinput-based injector) for automatic paste, and falls back to leaving the
> entry on the clipboard with a "press Ctrl+V" notification when ydotool is not
> available.

## Build & run

Prerequisites (Debian/Ubuntu/KDE Neon package names; adjust for your distro):

```bash
sudo apt install \
  libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev \
  build-essential curl wget file
```

Then, from the repository root:

```bash
npm install
npm run tauri dev      # development
npm run tauri build    # produces .deb / .rpm / AppImage under src-tauri/target/release/bundle/
```

Runtime extras (recommended, installed automatically by the `.deb`):

```bash
sudo apt install ydotool libnotify-bin
```

## System tray

On launch Copywraith adds a StatusNotifierItem to the Plasma tray. Right-click
it for:

- **Show Copywraith** / **Show Starred** — open the popup.
- **Paste last entry as plain text**.
- **Start at login** — toggles an XDG autostart entry (see below).
- **Quit Copywraith**.

The popup window starts hidden; the tray icon (and global shortcuts) are how you
summon it.

## Automatic paste (Wayland) via ydotool

`ydotool` talks to `/dev/uinput`, which needs a running daemon and permission to
that device:

1. Install `ydotool` (and `ydotoold`).
2. Enable the daemon. Many distros ship a user service:
   ```bash
   systemctl --user enable --now ydotoold
   ```
   If your packaging doesn't include it, run `ydotoold` once to confirm it works.
3. Make sure your user can access `/dev/uinput` (commonly via an `input` group
   and a udev rule):
   ```bash
   sudo usermod -aG input "$USER"   # then log out and back in
   ```

When `ydotool` is present and working, tapping an entry in the popup copies it
and immediately pastes into the previously focused window. Otherwise the entry is
copied and you press **Ctrl+V** yourself.

## Global shortcuts (KDE-native)

Wayland doesn't let apps grab global hotkeys, so Copywraith exposes its actions
on the command line and uses a single-instance guard: running the binary again
forwards the command to the instance already in the tray. Bind KDE shortcuts to
these commands:

| Command | Action |
| --- | --- |
| `copywraith --toggle` | Toggle the popup |
| `copywraith --starred` | Toggle the popup filtered to starred entries |
| `copywraith --paste-plaintext` | Paste the most recent entry as plain text |

To bind them in Plasma 6:

1. **System Settings → Keyboard → Shortcuts → Add New → Command or Script…**
2. Enter the command (e.g. `copywraith --toggle`).
3. Assign a key combination (e.g. `Meta+V`).
4. Repeat for the other commands.

These live in KDE's own shortcut configuration and survive reboots. (On an X11
session the app's built-in global-shortcut registration also works, but the
command approach above is the reliable cross-session option.)

## Autostart

Toggle **Start at login** in the tray menu, or manage it yourself — Copywraith
writes/removes:

```
~/.config/autostart/copywraith.desktop
```

## Packaging notes

`npm run tauri build` emits a `.deb`, `.rpm`, and AppImage. The `.deb` declares
runtime dependencies on WebKitGTK, GTK 3, and the Ayatana AppIndicator library,
and recommends `ydotool` + `libnotify-bin` for the full paste experience.

## Troubleshooting

- **Paste does nothing, no notification**: `ydotool` ran but the keystroke went
  nowhere — confirm the target window had focus, and that `ydotoold` is running.
- **"press Ctrl+V" notification every time**: `ydotool` isn't installed or can't
  reach `/dev/uinput`. See the ydotool section above.
- **Popup doesn't take focus on Wayland**: KDE's focus-stealing prevention can
  hold the window back; click the tray item again, or lower focus-stealing
  prevention in System Settings → Window Management.
- **Tray icon missing**: ensure `libayatana-appindicator3-1` is installed and the
  Plasma "System Tray" widget is shown.
</content>
