# Window Notes (macOS Spaces / Fullscreen)

Date: 2026-03-26

## Problem recap

- Popup positioning near cursor is now mostly correct.
- In macOS fullscreen apps, the popup was appearing in an adjacent non-fullscreen Space instead of the active fullscreen Space.

## Findings from docs / ecosystem review

- Plain Tauri window settings like `visibleOnAllWorkspaces` / `set_visible_on_all_workspaces(true)` are usually not sufficient for reliable overlay behavior in fullscreen Spaces.
- The common working pattern for "Spotlight-like" overlays is to use `NSPanel` with fullscreen-related collection behavior flags.
- Official Tauri plugins list does not include a dedicated fullscreen-Space overlay plugin.
- Relevant community plugins:
  - `tauri-nspanel` (v2) - convert `WebviewWindow` (`NSWindow`) to `NSPanel`.
  - `tauri-plugin-spotlight` (has a Tauri v2 branch) - also panel-based approach.
  - `tauri-plugin-nspopover` - menu bar popover pattern (different UX model).
- `tauri-nspanel` includes a fullscreen example that explicitly sets:
  - non-activating panel style mask,
  - `FullScreenAuxiliary`,
  - `CanJoinAllSpaces`,
  - and typically a high window level.

## Important implementation pitfalls found earlier

- Direct custom Objective-C interop added manually caused hard crashes (`panic in a function that cannot unwind` / `Rust cannot catch foreign exceptions`), likely from foreign exceptions crossing FFI boundaries.
- Re-show/re-focus fallbacks can destabilize toggle behavior and create "duplicate window" symptoms.
- Converting the popup to `NSPanel` too early in app startup can panic/abort when the popup window is still hidden/not fully realized.
- Panics from third-party panel conversion code must be caught inside the main-thread task; if they unwind past runtime callback boundaries, the process aborts (`panic in a function that cannot unwind`).
- Objective-C exceptions thrown during panel conversion/configuration cannot be caught by Rust (`Rust cannot catch foreign exceptions`) and will abort the process.
- In fullscreen mode, panel can flash then disappear immediately if it auto-hides on deactivate or if shortcut key-repeat triggers a second toggle.

## Implemented option 1 (NSPanel migration)

Implemented in this repo:

- Added macOS-only dependency:
  - `src-tauri/Cargo.toml`
  - `tauri-nspanel = { git = "https://github.com/ahkohd/tauri-nspanel", branch = "v2" }`
- Registered plugin on macOS:
  - `src-tauri/src/lib.rs` -> `builder.plugin(tauri_nspanel::init())`
- Converted popup window to `NSPanel` during setup and applied fullscreen flags:
  - `ensure_popup_panel_for_fullscreen_spaces()` in `src-tauri/src/lib.rs`
  - conversion/configuration now runs lazily on first popup open, on the main thread, then caches completion in app state
  - conversion now runs before first `show()` call (while hidden) to reduce class-swizzle instability
  - low-level function: `configure_popup_panel_for_fullscreen_spaces_now()`
  - conversion now checks `popup.ns_window()` first to avoid conversion before native handle exists
  - conversion wrapper uses `catch_unwind` inside the main-thread task; on panic, panel mode is disabled for that run (avoid hard abort)
  - panel configuration now mirrors `tauri-nspanel` fullscreen example as closely as possible:
    - non-activating style mask only
    - collection behavior = `FullScreenAuxiliary | CanJoinAllSpaces`
    - removed extra tweaks (`MoveToActiveSpace`, floating/hide flags) for stability
  - then re-introduced one targeted stability tweak: `panel.set_hides_on_deactivate(false)` to prevent instant disappear in fullscreen.

## Latest toggle/flicker fix attempt

- Toggle path now prefers panel visibility (`panel.is_visible()`) on macOS when panel exists.
  - close: `panel.order_out(None)`
  - open: position via popup API, then `panel.show()`
- Added debounce (`180ms`) in backend toggle path to avoid global shortcut key-repeat causing open->close immediately.
- Added an extra guard: if panel became visible <350ms ago, ignore close requests as likely key-repeat.
- Non-macOS path still uses normal `popup.show()` / `popup.hide()`.
- Shortcut trigger state for popup toggles changed from `Pressed` to `Released` to avoid repeated key-down events causing open->close flicker while modifier keys are still held.
- Added conflict guard: if toggle and starred-popup shortcuts are configured identically, starred registration is skipped (with warning) to prevent double-callback open+close behavior.

## Close button / panel consistency fix

- The in-window close button previously called frontend `appWindow.hide()`, which could diverge from panel visibility state on macOS.
- Added backend command `hide_popup` that hides both panel (if present) and popup window.
- Frontend `WindowManager.close()` now calls `TauriService.hidePopup()` first (with fallback to `appWindow.hide()` on error).
- Paste flow now also uses shared backend hide helper so panel/window visibility stays consistent after a paste action.

## Critical crash finding

- Calling NSPanel methods directly (`panel.order_out` / `panel.show`) from paths that may run off the macOS main thread can trigger Objective-C exceptions and abort the process (`Rust cannot catch foreign exceptions`).
- Current mitigation: keep NSPanel conversion/config on main thread, but use regular Tauri window APIs (`popup.show()` / `popup.hide()`) for runtime open/close actions.

## Reliability follow-up adjustments

- Added backend `popup_open` atomic state to track intended popup visibility instead of relying only on `is_visible()` / `is_focused()` (which can be inconsistent with panel behavior across Spaces).
- Runtime panel show/hide calls were reintroduced, but only via `popup.run_on_main_thread(...)` helpers:
  - `request_panel_show_on_main_thread()`
  - `request_panel_hide_on_main_thread()`
- Shared hide helper (`hide_popup_window`) now:
  - requests panel hide on main thread (macOS)
  - hides popup window
  - updates `popup_open = false`
  - Sets:
    - window level to `NSMainMenuWindowLevel + 1`
    - style mask to non-activating + resizable
    - collection behavior:
      - `FullScreenAuxiliary`
      - `CanJoinAllSpaces`
      - `MoveToActiveSpace`
- Updated toggle path on macOS to use panel show/hide when available:
  - popup still uses normal Tauri show/hide for stability
  - NSPanel conversion is a one-time enhancement step after first successful show.

## Caveats

- `to_panel()` should only be called once per window lifecycle.
- Avoid maximizing/fullscreening the panel window directly.
- If behavior regresses, check whether panel conversion succeeded in startup logs.

## If we need to revert quickly

1. Remove macOS dependency from `src-tauri/Cargo.toml`.
2. Remove plugin registration `tauri_nspanel::init()`.
3. Remove `configure_popup_panel_for_fullscreen_spaces`, `show_popup_panel_if_available`, `hide_popup_panel_if_available` from `src-tauri/src/lib.rs`.
4. Restore `toggle_popup` to plain `popup.show()` / `popup.hide()`.
