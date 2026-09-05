# Copywraith on KDE / Linux

The desktop app is built from the same Tauri + Svelte codebase as the macOS
build. This page covers the KDE Plasma (Wayland-first) integration: system tray,
paste injection, global shortcuts, and autostart.

> [!NOTE]
> On Ubuntu/GNOME, read [`README.ubuntu.md`](README.ubuntu.md) instead — there
> Copywraith registers the global shortcuts with GNOME for you.

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

Copywraith registers three native KGlobalAccel actions on Plasma X11 and
Wayland. While Copywraith runs, open **System Settings → Keyboard → Shortcuts →
Copywraith** to assign or disable them:

- Toggle popup
- Starred popup
- Paste as plain text

New actions start unbound to avoid desktop conflicts. Existing KDE assignments,
including disabled actions, survive registration and daemon restarts. KDE owns
these bindings; Copywraith's accelerator fields apply to other desktops.
Copywraith Settings shows connection status and a **Refresh status** button.
It retries automatically if the service disconnects.

Command shortcuts remain available as a fallback:

| Command | Action |
| --- | --- |
| `copywraith --toggle` | Toggle the popup |
| `copywraith --starred` | Toggle the starred popup |
| `copywraith --paste-plaintext` | Paste the most recent entry as plain text |

Bind commands using **Add New → Command or Script…**. Avoid assigning the same
key to both a command shortcut and a native action. Copywraith must be running
for native actions to work; enable **Start at login** if needed.

When automatic paste fails, Copywraith sends native desktop guidance to press
**Ctrl+V** without reopening or focusing the popup. This uses the desktop's
notification service directly; `notify-send` is only a fallback. Notification
visibility still follows the desktop's notification policy (including Do Not
Disturb).

### Isolated validation

`scripts/test-kde.sh` runs the production D-Bus adapters against mock services on
an isolated bus. `scripts/test-kde-runtime.sh` additionally requires KGlobalAccel,
Xvfb, xdotool, dunst and xterm. It creates a disposable display, session bus and
XDG directories, tests actual key delivery and saved assignments after restart,
and checks visible native guidance without moving focus from a target window.
It never connects to the host display or session bus. The notification runtime
check uses dunst; visual verification in Plasma's notification shell remains a
separate check.

## Autostart

Toggle **Start at login** in the tray menu, or manage it yourself — Copywraith
writes/removes:

```
~/.config/autostart/copywraith.desktop
```

## Packaging notes

`npm run tauri build` emits a `.deb`, `.rpm`, and AppImage. The `.deb` installs
the binary as `/usr/bin/copywraith` (which is what the shortcut commands above
call), declares runtime dependencies on WebKitGTK, GTK 3, and the Ayatana
AppIndicator library, and recommends `ydotool` + `libnotify-bin` for the full
paste experience.

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
