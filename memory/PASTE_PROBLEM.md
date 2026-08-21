# Paste Simulation Problems on macOS

## Observed Symptom

1. User focuses a text field in another app (e.g., TextEdit, Chrome).
2. User presses Cmd+Shift+V to open the Copywraith popup.
3. User clicks an entry in the popup.
4. The popup closes; after ~200ms the original app regains focus.
5. **Nothing is pasted into the text field.**

## Paste Flow (before fixes)

```
toggle_popup()
  -> remember_frontmost_app()          # osascript: detect frontmost process
  -> popup.show() / set_focus()

User clicks entry -> EntryRow.handleClick()
  -> pasteEntry(id)                    # JS invoke
  -> Rust paste_entry command
       -> write_and_paste_text(app, text)
            1. preferred_paste_target() # read remembered app
            2. clipboard.write_text()   # *** TRIGGERS CLIPBOARD MONITOR ***
            3. popup.hide()
            4. simulate_paste()         # spawn thread: sleep 100ms, osascript
```

## Identified Problems

### Problem 1 — Clipboard monitor feedback loop (BUG)

`write_and_paste_text` (and `write_and_paste_image`) write to the system
clipboard to prepare for pasting.  This write triggers the clipboard monitor
(`plugin:clipboard://clipboard-monitor/update`), which:

1. Calls `detect_source_app_name()` — spawns **another** `osascript` process.
2. Reads all clipboard flavours (`has_image`, `has_html`, `has_text`, ...).
3. Stores the entry (usually a duplicate, but see below).

**Why this is harmful:**

* Two concurrent `osascript` processes talk to System Events at the same time
  (the monitor's `detect_source_app_name` and the paste simulation's keystroke
  script).  On some system configurations this creates contention that can delay
  or drop the simulated keystroke.
* For **HTML entries** specifically, `paste_entry` strips tags and writes
  **plain text** to the clipboard.  The monitor sees this as new content
  (different `content_hash`) and creates a **spurious Text entry** in the
  history.
* The monitor's reads, while normally non-destructive, add unnecessary
  processing during a time-critical window (between clipboard write and
  simulated paste).

**Source:** `src-tauri/src/paste.rs:32` (write), `src-tauri/src/clipboard.rs:37-41` (monitor).

**Fix:** Add an `AtomicBool` suppress flag to `AppState`.  Set it **before**
the clipboard write; the monitor checks-and-resets it, skipping processing for
self-triggered events.

---

### Problem 2 — Paste simulation errors are invisible (BUG)

`simulate_paste()` uses `Command::new("osascript").status()`, which only
captures the exit code.  When the paste fails (most commonly due to missing
Accessibility permission), the actual error message from `osascript`'s stderr
is silently discarded:

```
execution error: System Events got an error: osascript is not allowed
assistive access. (-1719)
```

The error is logged via `log::error!` with just the exit code, but:
* The user never sees log output in the UI.
* The message "exited with status: 1" gives no actionable guidance.

**Source:** `src-tauri/src/paste.rs:86-102`.

**Fix:** Switch to `Command::output()` to capture stderr; parse it for known
error patterns; emit a Tauri event (`paste-failed`) so the frontend can show
a notification.

---

### Problem 3 — No Accessibility permission check (MISSING)

The `keystroke` command sent via System Events requires the calling process
to have **Accessibility** access (System Settings > Privacy & Security >
Accessibility).  Without it:

* `tell application "X" to activate` **works** (normal AppleScript, no special
  permissions required) — this is why the target app regains focus.
* `tell application "System Events" to keystroke "v" using command down`
  **silently fails** — the keystroke is never delivered.

This matches the exact symptom reported: focus returns, paste does not.

macOS provides `AXIsProcessTrusted()` (ApplicationServices framework) to
check this at runtime.  The app currently never calls it.

**Fix:** Before the paste simulation, check `AXIsProcessTrusted()`.  If false,
log a warning — but **do not bail out early**.  The osascript must still run so
that the `activate` line restores focus to the target app.  Only `keystroke`
requires Accessibility; it will fail, and the stderr-capture code (Problem 2)
surfaces the error to the user.

> **Note:** An earlier version of this fix used an early `return` when
> Accessibility was not granted.  That was wrong — it prevented the `activate`
> command from running, so the target app never regained focus at all.  The
> corrected version logs a warning and lets the osascript proceed.

---

### Problem 4 — Post-activate delay may be too short (MINOR)

After `tell application "X" to activate`, the script waits only `delay 0.08`
(80 ms) before sending the keystroke.  For heavyweight apps or under load,
80 ms may not be enough for the target app's run loop to process the
activation and be ready to receive synthetic key events.

**Source:** `src-tauri/src/paste.rs:83`.

**Fix:** Increase the post-activate delay from 80 ms to 140 ms.

---

### Problem 5 — App activation failure could cancel paste (EDGE CASE)

`detect_frontmost_app_name()` returns the **System Events process name**
(e.g. `"Code"` for VS Code).  `tell application "X" to activate` resolves
`X` via Launch Services, which usually matches but can differ for some apps.
When activation fails, AppleScript can abort before running Cmd+V.

This is especially bad in this flow because the popup is already hidden,
so the user sees no obvious error and just gets "nothing pasted".

**Fix:** Wrap the activation line in `try ... end try` so activation errors do
not prevent the script from attempting Cmd+V.

---

### Problem 6 — Failure feedback was hidden with the popup (BUG)

Paste failures emit a `paste-failed` event for the frontend, but the popup is
hidden before paste simulation starts.  This means the notification UI may not
be visible exactly when it is needed.

**Fix:** On paste failure, re-show/focus the popup and then emit `paste-failed`
so the notification is visible.

---

### Problem 7 — Some apps are flaky with literal `keystroke "v"` (EDGE CASE)

On some apps/environments, `keystroke "v" using {command down}` can fail even
when Accessibility permission is present.

**Fix:** Add a fallback that retries paste via key-code simulation:
`key code 9 using {command down}`.

---

### Problem 8 — Synchronous `simulate_paste` causes paste regression (BUG)

`simulate_paste` must run in a spawned thread, not inline on the Tauri async
runtime.  When it runs synchronously:

1. The `paste_entry` IPC call blocks for ~400ms (100ms sleep + osascript
   execution with 140ms delay), preventing the frontend from responding.
2. `hide_popup_window` dispatches the popup hide to the main thread
   asynchronously and then immediately calls `restore_previous_focus` (which
   spawns an osascript in a thread) **before** `simulate_paste` runs.
3. Because `simulate_paste` is synchronous, its own osascript runs in-line
   with the Tauri runtime instead of concurrently, changing the timing so the
   Cmd+V keystroke can arrive before the target app has been re-activated.

**Symptom:** Popup closes, previous app regains focus, but nothing is pasted.

**Source:** `src-tauri/src/paste.rs:147` (`simulate_paste` function).

**Fix:** Wrap the macOS paste body in `std::thread::spawn(move || { ... })`
instead of a bare `{ ... }` block.  This lets the `paste_entry` command return
immediately while the paste simulation runs on its own thread with proper
timing.

---

## Summary of Fixes Applied

| # | Problem | Fix | Files Changed |
|---|---------|-----|---------------|
| 1 | Clipboard monitor feedback loop | Suppress window (`suppress_monitor_until`) | `lib.rs`, `clipboard.rs`, `paste.rs` |
| 2 | Invisible paste errors | `.output()` + stderr parsing + `paste-failed` event | `paste.rs`, `+page.svelte` |
| 3 | No Accessibility check | `AXIsProcessTrusted()` FFI preflight warning (no early return) | `paste.rs` |
| 4 | Short post-activate delay | 80 ms -> 140 ms | `paste.rs` |
| 5 | Activation failure aborting paste | `try ... end try` around `activate` | `paste.rs` |
| 6 | Failure feedback hidden while popup closed | Re-show popup on paste failure | `paste.rs`, `+page.svelte` |
| 7 | Literal keystroke flakiness | Fallback `key code 9` retry path | `paste.rs` |
| 8 | Synchronous `simulate_paste` regression | Spawn thread instead of inline block | `paste.rs` |

### Verification

* `cargo check --workspace` -- passes with no warnings.
* `npm run build` -- Svelte frontend builds cleanly.
* `cargo test -p copywraith-core` -- 32/32 tests pass.

### What the user needs to do

After deploying these fixes, if paste still does not work the most likely
remaining cause is **missing Accessibility permission**.  The app will now
re-open the popup and show a red notification banner with instructions.  The
user should:

1. Open **System Settings > Privacy & Security > Accessibility**.
2. Find and enable **Copywraith** (or the dev build name).
3. If it was already listed, toggle it off and on again.
4. Re-try the paste.
