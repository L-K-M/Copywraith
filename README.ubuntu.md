# Copywraith on Ubuntu / GNOME

The desktop app is built from the same Tauri + Svelte codebase as the macOS and
KDE builds. This page covers the Ubuntu default session — GNOME on Wayland —
where global shortcuts work differently from every other platform.

> [!NOTE]
> If you run KDE Plasma, read [`README.kde.md`](README.kde.md) instead. The
> build and paste sections are the same; only the shortcut mechanism differs.

## Install

Download the `.deb` or the AppImage from the
[latest release](https://github.com/L-K-M/Copywraith/releases/latest):

```bash
sudo apt install ./Copywraith_*_amd64.deb
```

The `.deb` installs `/usr/bin/copywraith`, a desktop entry, and icons, and pulls
in WebKitGTK, GTK 3, and the Ayatana AppIndicator library. It also *recommends*
`ydotool` and `libnotify-bin`, which Ubuntu installs by default with
`apt install`; they power automatic paste and its fallback notification.

To build from source instead:

```bash
sudo apt install \
  libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev \
  build-essential curl wget file libxdo-dev libssl-dev

npm install
npm run tauri dev      # development
npm run tauri build    # .deb / .rpm / AppImage under target/release/bundle/
```

## Global keyboard shortcuts

Wayland deliberately forbids applications from grabbing keys globally, and the
Tauri global-shortcut plugin only has an X11 backend on Linux. On a Wayland
session it reports success and then never fires — so Copywraith does not rely on
it there.

Instead, **Copywraith registers your shortcuts with GNOME itself**. Set them in
**Settings → Keyboard Shortcuts** as usual (`CmdOrCtrl+Shift+V` and friends) and
on save the app writes matching GNOME *custom keybindings* that run:

| Command | Action |
| --- | --- |
| `copywraith --toggle` | Toggle the popup |
| `copywraith --starred` | Toggle the popup filtered to starred entries |
| `copywraith --paste-plaintext` | Paste the most recent entry as plain text |

Running the binary again does not start a second copy: a single-instance guard
forwards the command over a Unix socket to the instance already in the tray.

You can see the result in **GNOME Settings → Keyboard → View and Customize
Shortcuts → Custom Shortcuts**, where the entries appear as *Copywraith: Toggle
popup* and so on. Copywraith updates those same rows when you change a shortcut
and removes them when you clear the field, so edit them from Copywraith's
Settings rather than in place.

The Settings dialog reports which mechanism is in use underneath the shortcut
fields:

- **GNOME custom keybindings** — the Wayland path described above.
- **Nothing shown** — an X11 session, where Copywraith grabs the keys itself
  and no external configuration is needed. (Switching from Wayland to X11
  removes the GNOME keybindings, so a shortcut never fires twice.)
- **A warning listing commands** — a Wayland session that is not GNOME, or a
  GNOME one whose settings could not be written. Bind the listed commands in
  your own keyboard settings.

### Shortcut format

Shortcuts use the Tauri accelerator syntax, e.g. `CmdOrCtrl+Shift+V`,
`Super+V`, `Alt+Space`, `Ctrl+F12`. On Linux `CmdOrCtrl` means Control and
`Super`/`Meta` mean the Super (Windows) key. Copywraith translates them into the
GTK form GNOME stores (`<Control><Shift>v`). If a combination cannot be
translated, Settings says so instead of leaving a shortcut that looks bound but
does nothing.

## System tray

Copywraith adds a StatusNotifierItem on launch. Ubuntu ships the AppIndicator
GNOME Shell extension enabled by default, so the icon appears in the top bar.
Right-click it for:

- **Show Copywraith** / **Show Starred** — open the popup.
- **Paste last entry as plain text**.
- **Start at login** — writes `~/.config/autostart/copywraith.desktop`.
- **Quit Copywraith**.

The popup window starts hidden; the tray icon and the global shortcuts are how
you summon it.

If the icon is missing, make sure `libayatana-appindicator3-1` is installed and
the *Ubuntu AppIndicators* extension is enabled (`gnome-extensions list`).

## Automatic paste (Wayland) via ydotool

Wayland does not let apps inject keystrokes either, so paste goes through
[`ydotool`](https://github.com/ReimuNotMoe/ydotool), which writes to
`/dev/uinput`:

```bash
sudo apt install ydotool
systemctl --user enable --now ydotoold
sudo usermod -aG input "$USER"   # then log out and back in
```

With `ydotool` working, tapping an entry copies it and pastes into the window
that had focus. Without it, the entry is copied and a notification asks you to
press **Ctrl+V** yourself.

## Troubleshooting

- **A shortcut does nothing**: open Settings and read the line under the
  shortcut fields — it names the mechanism actually in use. On GNOME, check
  **Keyboard → Custom Shortcuts** for a *Copywraith:* row, and make sure the
  combination is not already claimed by GNOME (it wins).
- **The shortcut starts a second window instead of toggling**: the
  single-instance socket lives in `$XDG_RUNTIME_DIR`. If that variable is unset
  the app falls back to `/tmp`; a session that changes it between launches
  breaks the handoff.
- **Paste does nothing, no notification**: `ydotool` ran but the keystroke went
  nowhere — confirm the target window had focus and that `ydotoold` is running.
- **"Press Ctrl+V" notification every time**: `ydotool` isn't installed or can't
  reach `/dev/uinput`. See above.
- **The popup opens in the wrong place**: Wayland does not let clients position
  their own windows, so the popup lands wherever the compositor puts it. On X11
  it appears next to the cursor.
