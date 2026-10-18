# Changelog

## Unreleased

Earlier history lives in the commit log and any GitHub releases.

### Added

- Pause clipboard capture for 5 minutes, 1 hour or until resumed from the
  status bar. A restart always resumes capture. (#161)
- Popup keyboard shortcuts: Mod+1 to Mod+9 paste a row, Mod+S stars,
  Mod+Y previews, Mod+Backspace deletes, Shift+Enter or Alt+Enter pastes as
  plain text, and Page Up/Down, Home and End move the selection. Mod is Cmd
  on macOS and Ctrl elsewhere. (#158)

### Changed

- The Android app is laid out for a phone. It has a System 7 title bar,
  copied text uses the same 24px font as the desktop popup, and the type,
  age and action columns are only as wide as their contents, so the text
  column gets about 60% more room. Rows are 44px touch targets, a tap
  flashes the row instead of leaving it highlighted, and ages older than
  30 days read "5mo" or "2y". The keyboard's search key closes the
  keyboard instead of copying the first row.
- On Android the Entry Preview and Settings dialogs use the System 7 fonts
  at readable sizes. Preview's buttons are a 2x2 grid labelled Copy and
  Copy as Text, and Settings keeps Cancel and Save pinned below its
  scrolling form.
- Content that a password manager marks as concealed, transient or
  auto-generated is no longer captured or synced. (#147)
- The sync status reports a rejected password or a server error instead of
  showing "online", and a push batch stops at the first rejected password.
  (#148)
- Sync starts at launch and checks for changes with a one-entry request, so
  an idle server no longer sends its newest 100 entries every 5 seconds.
  (#149)
- The server gzip-compresses API and admin UI responses, and the clients
  accept them. Images and file blobs are sent as they are. (#150)
- Starring responds immediately; the change syncs in the background. (#153)
- Changing the server URL makes the next sync re-read the new server's
  history instead of skipping entries older than the old server's cursor.
  (#154)
- Copying several files keeps all of them instead of storing only the first
  image. Linux file URIs with spaces or other escaped characters are decoded
  and pasted correctly. (#155)
- Sync deadlines scale with the size of each upload or download, so large
  images and files no longer fail after 30 seconds. (#157)
- Pasting as plain text keeps the copied indentation and trailing newlines.
  (#159)

### Fixed

- On an Android phone in dark mode the status bar clock and battery, and
  the navigation buttons, were drawn white on Copywraith's white window
  and could not be seen. They are now dark (the navigation buttons from
  Android 8 on; Android 7 has no dark navigation buttons and keeps its
  dark bar behind them).
- On Android, Entry Preview's buttons no longer run off the left edge of
  the screen, toasts no longer cover the status bar's Sync button, and
  the status bar's grey reaches the bottom edge under the gesture bar.
- On Android, reloading the list on every resume and sync no longer
  replaces it with "Loading clipboard..." and scrolls back to the top.
- `scripts/build.sh` stops with a clear message when Node is older than a
  package's `engines` range, instead of failing in vite with "Cannot find
  native binding", and reinstalls npm dependencies after a Node upgrade. A
  new `.nvmrc` selects Node 22; when nvm is installed the script switches to
  it automatically. Missing Android Rust targets are added with rustup, and
  `scripts/android-dev-bootstrap.sh` installs all four instead of only
  aarch64.
- Server setup and password change reject passwords that cannot be sent in
  an HTTP header (non-ASCII, or a leading or trailing space), which
  previously locked every client out. (#152)
- Android no longer imports `file://` shares or shares from Copywraith's own
  providers, and a shared file's name is reduced to its final component.
  (#151)
- The server's text-flavor backfill runs once per database instead of on
  every start. (#156)
- The Docker build context no longer includes live server data, `.env`
  files, `node_modules` or build output. (#162)

### Dependencies

- argon2 0.6 on the server; Vite 8.3 and Vitest 5 for the admin UI; Vite 8.3
  and @types/node 26.6 for the popup; actions/setup-java 6.0.1. CI, releases
  and the Docker UI build stage use Node 22. (#163)
- @lkmc/system7-ui 0.3.0 for the popup and the admin UI. Dialog title bars
  keep their full height instead of being squashed when a dialog is taller
  than the window, and the shaded popup is 39px tall so the title bar is not
  clipped. Dialogs keep their 12px margin and Android toasts their position
  through the library's new safe-area and toast offset tokens.
