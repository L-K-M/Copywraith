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
