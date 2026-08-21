# Cross-Platform Paste Simulation: Research & Recommendations

## Executive Summary

This document evaluates options for implementing paste simulation on Windows and Linux without breaking macOS. It draws on research of existing Tauri clipboard managers and their approaches.

## Current State (macOS Only)

Copywraith currently implements paste simulation only for macOS using `osascript` + AppleScript:

```rust
// src-tauri/src/paste.rs
Command::new("osascript")
    .arg("-e")
    .arg("tell application \"System Events\" to keystroke \"v\" using command down")
```

Non-macOS platforms log a warning and do nothing:
```rust
#[cfg(not(target_os = "macos"))]
{
    log::warn!("Simulated paste is not implemented on this platform");
}
```

---

## How Other Tauri Clipboard Managers Handle This

### 1. EcoPaste (Most Comprehensive)

EcoPaste has a dedicated `tauri-plugin-eco-paste` with platform-specific implementations:

| Platform | Keyboard Simulation | Window Tracking |
|----------|---------------------|-----------------|
| macOS | `osascript` + AppleScript | Cocoa `NSWorkspaceDidActivateApplicationNotification` |
| Windows | `enigo` crate | WinAPI `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` |
| Linux | `rdev` crate | X11 `XGetInputFocus` + `XSelectInput` |

**Key insight**: EcoPaste uses **Shift+Insert** instead of Ctrl+V/Cmd+V on Windows and Linux:

```rust
// Windows
enigo.key(Key::Shift, Press).unwrap();
enigo.key(Key::Other(0x2D), Click).unwrap();  // 0x2D = VK_INSERT
enigo.key(Key::Shift, Release).unwrap();

// Linux
dispatch(&EventType::KeyPress(Key::ShiftLeft));
dispatch(&EventType::KeyPress(Key::Insert));
dispatch(&EventType::KeyRelease(Key::Insert));
dispatch(&EventType::KeyRelease(Key::ShiftLeft));
```

### 2. PasteBar

- Uses custom `inputbot` library (local fork)
- Windows-specific: `clipboard-win`, `winapi`
- macOS: `cocoa`, `objc`, `macos-accessibility-client`
- Linux: `inputbotlinux` (local fork)

### 3. Qopy

- Uses `rdev` crate for keyboard simulation
- Uses `tauri-plugin-clipboard` (same as Copywraith)

---

## Recommended Libraries

### Option A: `enigo` (Recommended for Windows/macOS)

- **Platforms**: Windows, macOS, Linux (X11 only)
- **Pros**:
  - Cross-platform API with consistent interface
  - Well-maintained, active development
  - Supports both keyboard and mouse simulation
  - Serde support for serialization
- **Cons**:
  - Linux requires X11 (no Wayland support)
  - macOS still requires Accessibility permission

```rust
// Example paste with enigo
use enigo::{Enigo, Key, Keyboard, Settings, Direction::{Click, Press, Release}};

let mut enigo = Enigo::new(&Settings::default()).unwrap();
enigo.key(Key::Control, Press).unwrap();
enigo.key(Key::Unicode('v'), Click).unwrap();
enigo.key(Key::Control, Release).unwrap();
```

### Option B: `rdev` (Recommended for Linux)

- **Platforms**: Windows, macOS, Linux (X11)
- **Pros**:
  - Simpler API than enigo
  - Also supports event listening
  - Used by EcoPaste and Qopy
- **Cons**:
  - Linux requires X11 (no Wayland support)
  - macOS requires Accessibility permission

```rust
// Example paste with rdev
use rdev::{simulate, EventType, Key};

fn dispatch(event_type: &EventType) {
    simulate(event_type).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
}

dispatch(&EventType::KeyPress(Key::ShiftLeft));
dispatch(&EventType::KeyPress(Key::Insert));
dispatch(&EventType::KeyRelease(Key::Insert));
dispatch(&EventType::KeyRelease(Key::ShiftLeft));
```

### Option C: External Tools (Fallback)

| Platform | Tool | Command |
|----------|------|---------|
| Linux (X11) | `xdotool` | `xdotool key --clearmodifiers ctrl+v` |
| Linux (Wayland) | `ydotool` | `ydotool key 125:1 47:1 47:0 125:0` (Ctrl+V) |
| Windows | PowerShell | `[SendKeys]::SendWait("^v")` |

---

## Platform-Specific Considerations

### Windows

1. **Keyboard shortcut**: Use `Ctrl+V` (not Cmd)
2. **Window focus**: Use `SetForegroundWindow()` via WinAPI
3. **Window tracking**: Use `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` to track previously focused window
4. **No special permissions required** for SendInput-style simulation

### Linux (X11)

1. **Keyboard shortcut**: Use `Ctrl+V` or `Shift+Insert`
2. **Window focus**: Use X11 `XRaiseWindow()` + `XSetInputFocus()`
3. **Window tracking**: Use X11 `XGetInputFocus()` + `XSelectInput(FocusChangeMask)`
4. **No special permissions required** for XTest extension

### Linux (Wayland)

1. **Major limitation**: Most keyboard simulation libraries don't work on Wayland
2. **Workaround options**:
   - `ydotool` (requires uinput access, may need root or group membership)
   - Portal-based solutions (e.g., `ashpd` for remote desktop portal)
   - Fall back to "copy only" mode (no paste simulation)

### macOS

1. **Keep current approach**: `osascript` is reliable and doesn't require native dependencies
2. **Continue using Accessibility check**: `AXIsProcessTrusted()`
3. **Alternative**: Could switch to `enigo` for consistency, but osascript already works

---

## Recommended Implementation Strategy

### Phase 1: Windows Support

Add conditional compilation for Windows using `enigo`:

```rust
// Cargo.toml
[target.'cfg(target_os = "windows")'.dependencies]
enigo = "0.3"

// paste.rs
#[cfg(target_os = "windows")]
fn simulate_paste(app: tauri::AppHandle, target_window: Option<isize>) {
    use enigo::{Enigo, Key, Keyboard, Settings, Direction::{Click, Press, Release}};
    
    if let Some(hwnd) = target_window {
        focus_window_windows(hwnd);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    let mut enigo = Enigo::new(&Settings::default()).unwrap();
    enigo.key(Key::Control, Press).unwrap();
    enigo.key(Key::Unicode('v'), Click).unwrap();
    enigo.key(Key::Control, Release).unwrap();
}
```

### Phase 2: Linux (X11) Support

Add conditional compilation for Linux using `rdev` or `enigo`:

```rust
// Cargo.toml
[target.'cfg(target_os = "linux")'.dependencies]
rdev = "0.5"
x11 = "2"

// paste.rs
#[cfg(target_os = "linux")]
fn simulate_paste(app: tauri::AppHandle, target_window: Option<u64>) {
    use rdev::{simulate, EventType, Key};
    
    if let Some(window) = target_window {
        focus_window_x11(window);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    fn dispatch(event_type: &EventType) {
        simulate(event_type).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    
    dispatch(&EventType::KeyPress(Key::ControlLeft));
    dispatch(&EventType::KeyPress(Key::KeyV));
    dispatch(&EventType::KeyRelease(Key::KeyV));
    dispatch(&EventType::KeyRelease(Key::ControlLeft));
}
```

### Phase 3: Wayland Fallback

For Wayland, fall back to "copy only" mode with user notification:

```rust
#[cfg(target_os = "linux")]
fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok() || 
    std::env::var("XDG_SESSION_TYPE").map(|s| s == "wayland").unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn simulate_paste(...) {
    if is_wayland() {
        // Wayland: copy only, no paste simulation
        emit_paste_failed(&app, "Paste simulation not supported on Wayland. Content copied to clipboard - press Ctrl+V manually.");
        return;
    }
    // X11 implementation...
}
```

---

## Window Tracking Architecture

Each platform needs its own mechanism to remember the previously focused window before showing the popup:

| Platform | Mechanism | Implementation |
|----------|-----------|----------------|
| macOS | `NSWorkspaceDidActivateApplicationNotification` | Already implemented via `remember_frontmost_app()` |
| Windows | `SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` | Store HWND in static `Mutex<Option<isize>>` |
| Linux | X11 `XSelectInput(FocusChangeMask)` | Store window ID in static `Mutex<Option<u64>>` |

### Shared Architecture

```rust
pub struct AppState {
    // Existing fields...
    
    #[cfg(target_os = "macos")]
    pub last_focused_app: std::sync::Mutex<Option<String>>,
    
    #[cfg(target_os = "windows")]
    pub last_focused_hwnd: std::sync::Mutex<Option<isize>>,
    
    #[cfg(target_os = "linux")]
    pub last_focused_window: std::sync::Mutex<Option<u64>>,
}
```

---

## Cargo.toml Changes

```toml
# Existing macOS-only dependencies
[target.'cfg(not(target_os = "android"))'.dependencies]
tauri-plugin-global-shortcut = "2"
tauri-plugin-clipboard = "2"

# Windows-specific: keyboard simulation + window tracking
[target.'cfg(target_os = "windows")'.dependencies]
enigo = "0.3"

# Linux-specific: keyboard simulation + X11 window tracking
[target.'cfg(target_os = "linux")'.dependencies]
rdev = "0.5"
x11 = "2"
```

---

## Risk Mitigation

1. **Don't break macOS**: Keep existing `osascript` implementation; don't replace with `enigo` on macOS
2. **Graceful degradation**: On failure, show notification that content was copied to clipboard
3. **Wayland handling**: Detect and inform user that paste simulation isn't supported
4. **Permission errors**: Surface clear error messages (similar to macOS Accessibility warnings)

---

## Testing Strategy

| Platform | Test Scenarios |
|----------|----------------|
| macOS | Existing tests; verify no regression |
| Windows | Focus various apps (Chrome, Notepad, Word), trigger paste |
| Linux X11 | Focus various apps, trigger paste; test multiple WMs (GNOME, KDE, i3) |
| Linux Wayland | Verify graceful fallback message |

---

## Summary Table

| Platform | Library | Keyboard Shortcut | Window Tracking | Special Requirements |
|----------|---------|-------------------|-----------------|---------------------|
| macOS (current) | `osascript` | Cmd+V | `osascript` + app name | Accessibility permission |
| macOS (alt) | `enigo` | Cmd+V | Same | Accessibility permission |
| Windows | `enigo` | Ctrl+V | WinAPI `SetWinEventHook` | None |
| Linux X11 | `rdev` or `enigo` | Ctrl+V | X11 `XSelectInput` | None |
| Linux Wayland | (none) | N/A | N/A | Fallback to copy-only |

---

## Files to Modify

1. `src-tauri/Cargo.toml` - Add platform-specific dependencies
2. `src-tauri/src/paste.rs` - Add Windows and Linux implementations
3. `src-tauri/src/lib.rs` - Update `AppState` with platform-specific window tracking
4. `src-tauri/src/clipboard.rs` - Potentially add window tracking initialization (Linux/Windows)
