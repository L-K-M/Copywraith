# macOS Paste Failure Analysis

## Scope

I reviewed the desktop paste path in:

- `src-tauri/src/lib.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/paste.rs`
- `src/routes/+page.svelte`

No code changes were made.

## What the current code does

1. `Cmd+Shift+V` opens the popup and stores a "last focused app" name (`remember_frontmost_app`) before showing the popup (`src-tauri/src/lib.rs:164`, `src-tauri/src/paste.rs:7`).
2. Clicking an entry calls `paste_entry`, which writes content to clipboard, hides the popup, then runs AppleScript to simulate `Cmd+V` (`src-tauri/src/commands.rs:108`, `src-tauri/src/paste.rs:30`, `src-tauri/src/paste.rs:53`, `src-tauri/src/paste.rs:94`).
3. Paste simulation is treated as successful when `osascript` exits with status 0 (`src-tauri/src/paste.rs:121`).

## Problems / possible reasons for "focus returns but nothing pastes"

### 1) Silent target-app capture failure can cause paste to be sent at the wrong time/app

- `detect_frontmost_app_name()` returns `None` on any `osascript` execution failure or non-success exit, with no logging (`src-tauri/src/paste.rs:289`, `src-tauri/src/paste.rs:296`).
- If target app is `None`, the script does not run `tell application "X" to activate`; it just sends `Cmd+V` after a fixed short wait (`src-tauri/src/paste.rs:181`, `src-tauri/src/paste.rs:206`).
- In that case, `Cmd+V` can fire while Copywraith is still the active app during hide/refocus transition, so nothing appears in the original text field even though focus returns shortly after.

### 2) Activation errors are intentionally swallowed, which can create false-success paste runs

- Activation is wrapped in AppleScript `try/end try` (`src-tauri/src/paste.rs:184`).
- If app-name resolution fails, script continues and still sends `Cmd+V` (`src-tauri/src/paste.rs:200`).
- This can yield exit status 0 ("success") while paste is effectively sent to the wrong context.

### 3) Fixed timing introduces a race with focus restoration

- The flow uses fixed delays (`sleep(100ms)` plus `delay 0.14` when target app exists) (`src-tauri/src/paste.rs:115`, `src-tauri/src/paste.rs:197`).
- If the destination app/text field is slower to regain first-responder status, the synthetic `Cmd+V` can arrive too early and be ignored.

### 4) Permissions can still block synthetic paste

- The keystroke path depends on macOS Accessibility trust (`AXIsProcessTrusted`) (`src-tauri/src/paste.rs:260`).
- If Accessibility or Apple Events control is denied, paste fails.
- The code does emit `paste-failed` on non-zero script failure (`src-tauri/src/paste.rs:156`, `src-tauri/src/paste.rs:251`), but this is a separate class of failure from the silent-success timing/target issues above.

### 5) Content-type edge cases can look like "nothing pasted"

- HTML entries are always converted with a very simple tag-stripper before paste (`src-tauri/src/commands.rs:129`, `src-tauri/src/commands.rs:307`).
- Some HTML payloads can reduce to near-empty plaintext, which can be perceived as failed paste even when keystroke delivery succeeded.

## Most likely match to your exact symptom

Given your report (popup closes, previous app regains focus, but no visible paste), the strongest code-level matches are:

1. target app not reliably activated (or activation silently failing), and/or
2. synthetic `Cmd+V` firing before the destination field is ready.

Those paths can produce a "successful" osascript run with no paste inserted.
