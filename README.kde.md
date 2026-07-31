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

Wayland doesn't let apps grab global hotkeys directly, so Copywraith integrates
with **KGlobalAccel** — KDE's own shortcut service — instead. There are two ways
to bind keys; the first is the recommended one.

### Recommended: assign keys in System Settings

While Copywraith is running it registers three actions with KGlobalAccel, so
they appear alongside every other app under **System Settings → Keyboard →
Shortcuts → Copywraith**:

| Action | What it does |
| --- | --- |
| Show clipboard history | Toggle the popup |
| Show starred entries | Toggle the popup filtered to starred entries |
| Paste last entry as plain text | Paste the most recent entry, stripped of formatting |

The actions ship **unbound** on purpose — a baked-in default like `Meta+V`
would collide with Klipper. Open that page, click each action, and assign a free
combination (e.g. `Meta+V` after freeing it from Klipper, or `Meta+Shift+V`).
The bindings live in KDE's config and survive reboots. Copywraith must be
running for a shortcut to fire; pair this with **Start at login** (below).

### Alternative: bind a command

Copywraith also accepts its verbs on the command line, forwarding them to the
running instance via a single-instance guard. This works on X11 too:

| Command | Action |
| --- | --- |
| `copywraith --toggle` | Toggle the popup |
| `copywraith --starred` | Toggle the popup filtered to starred entries |
| `copywraith --paste-plaintext` | Paste the most recent entry as plain text |

Bind them under **System Settings → Keyboard → Shortcuts → Add New → Command or
Script…**, entering e.g. `copywraith --toggle` and assigning a key.

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
