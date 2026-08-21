# Android Clipboard Sync Options

## Executive Summary

For a normal Android app on modern Android, Copywraith cannot reliably poll the system clipboard from the background and upload changes to the server. Since Android 10, clipboard contents are only available to the foreground app with focus and to the current default input method editor. A background service, WorkManager job, broadcast receiver, or Tauri background task does not bypass that restriction.

What is feasible:

- Capture the clipboard when Copywraith is opened, resumed, or otherwise in the foreground.
- Periodically sync local entries that have already been captured while the app process is alive.
- Use explicit user actions such as a share target, notification action, quick settings tile, or capture button.
- Build a keyboard/IME companion if automatic capture during text entry is worth the UX and trust cost.
- Add an optional Shizuku/Sui advanced mode for users willing to grant ADB shell or root-backed privileges.
- Use privileged/device-owner/root-only approaches for non-consumer deployments.

The recommended product path is to keep lifecycle capture as the baseline, add a foreground-only capture loop or Android clipboard listener for better in-app behavior, and optionally add explicit capture surfaces such as a share target or quick settings tile. True transparent background clipboard sync should not be treated as achievable for the standard Android app. Shizuku is the strongest advanced-user exception, but it should be treated as an opt-in privileged mode rather than the default Android design.

## Current Copywraith Android Behavior

Relevant files:

- `src-tauri/src/commands.rs` has `capture_clipboard`, which reads Android clipboard text with `tauri-plugin-clipboard-manager`, stores it locally, emits `clipboard-updated`, and triggers `sync_entry` for a new entry.
- `src/routes/+page.svelte` calls `refreshMobileEntries` when the mobile app opens and when the window regains focus. That function imports pending shares, captures the current clipboard, reloads local history immediately when capture inserts a new item, and then runs `sync_now`.
- `src-tauri/src/lib.rs` starts `start_sync_loop` on all platforms. That loop pushes unsynced local entries and pulls remote entries every 5 seconds while the app process is alive, with backoff on failures.
- `src-tauri/capabilities/mobile.json` grants `clipboard-manager:allow-read-text` and `clipboard-manager:allow-write-text`.

Important distinction:

- Android already has active-process periodic sync in this codebase, but it is not a durable Android background job. The OS may suspend or kill the app when it is backgrounded.
- Android does not have reliable background clipboard capture. The current capture model is lifecycle-triggered foreground capture.

## Android Platform Constraints

Clipboard access is constrained by Android itself, not mainly by Tauri.

- Android 10 and newer restrict clipboard reads to the currently focused app and the current default IME. Generic background apps cannot read clipboard contents.
- `ClipboardManager.OnPrimaryClipChangedListener` is useful only while the app is allowed to access the clipboard. Keeping a listener in a background process is not enough.
- There is no public system broadcast for clipboard changes. A `BroadcastReceiver` cannot subscribe to clipboard updates.
- `IntentService` is deprecated and does not solve clipboard access restrictions.
- WorkManager and AlarmManager can schedule background work, but scheduled work still cannot read clipboard data on modern Android unless the app is in an allowed state.
- A foreground service can keep work visible to the user, but it still does not make a generic app the foreground focused app or default IME.
- Shizuku changes the privilege identity of helper code to ADB shell or root, which can bypass some normal app limits. It does not make the main Copywraith app a normal background clipboard reader.
- On AOSP, the shell package has `android.permission.READ_CLIPBOARD_IN_BACKGROUND`, so Shizuku's ADB mode can plausibly read clipboard data in the background. OEM builds and Android versions can differ.
- Some privileged permissions, such as background clipboard access for system apps, are not available to ordinary Play Store or sideloaded user apps.
- Android 12 and newer show user-visible clipboard access indicators. Any aggressive polling strategy may feel invasive even when technically allowed.
- Android 13 and newer can auto-clear clipboard contents after a period of time, so delayed capture may miss entries.

## Option Matrix

| Option | Captures Clipboard While App Is Foreground | Captures Clipboard While Backgrounded on Android 10+ | Syncs Already Captured Entries in Background | Consumer-App Fit | Recommendation |
| --- | --- | --- | --- | --- | --- |
| Lifecycle capture on open/resume | Yes | No | Only while process remains alive | High | Keep as baseline |
| Foreground-only periodic polling | Yes | No | While foreground/process alive | High | Add if refresh feels stale |
| Android clipboard listener while foreground | Yes | No | While foreground/process alive | High | Prefer over polling if native work is acceptable |
| WorkManager periodic sync | No | No | Yes, with native implementation caveats | Medium | Useful for server sync, not clipboard capture |
| Foreground service monitor | Limited | No on modern Android for normal apps | Yes while service runs | Low | Avoid as primary strategy |
| Default keyboard/IME | Yes, while IME is active/default | Partially, via IME exception | Yes with native work | Medium/Low | Consider only as a power-user mode |
| Share target / explicit capture action | User-triggered | User-triggered | Can trigger immediate sync | High | Good complementary feature |
| Accessibility service | Not directly | Not reliably | Possible but policy-heavy | Low | Avoid |
| BroadcastReceiver / IntentService | No | No | Not appropriate | Low | Do not use |
| Shizuku / Sui privileged helper | Yes | Likely, if shell/root has clipboard permission | Yes, if helper uploads directly or can reach app IPC | Low for mainstream, high for power users | Best advanced-user route |
| Device-owner / privileged / root | Yes | Yes, if privileged enough | Yes | Very low for consumers | Special deployments only |
| Server push / FCM pull wakeup | No | No | Yes, for remote-to-device updates | Medium | Complementary only |

## Option 1: Lifecycle Capture on Open/Resume

### Description

Capture the clipboard when the Android app opens, resumes, or regains focus, then push any new entry to the server. This is the current Copywraith design.

The user copies something in another app, opens Copywraith later, and Copywraith reads the clipboard while it is foregrounded. If the content is new, it is stored locally and synced.

### Advantages

- Already mostly implemented in this repository.
- Complies with Android clipboard restrictions.
- No persistent notification, background service, or unusual permission prompt.
- Simple to explain to users: opening the app captures the current clipboard.
- Works well with the existing local-first model and content-hash deduplication.

### Drawbacks

- Not automatic while the app is backgrounded.
- Can miss clipboard contents that are overwritten before the user opens Copywraith.
- Can miss clipboard contents that Android auto-clears before the user opens Copywraith.
- Currently limited to text through `tauri-plugin-clipboard-manager`.
- Mobile capture is foreground/lifecycle-triggered, so Copywraith can still miss clipboard contents that are replaced before the user opens or resumes the app.

### Implementation Steps

1. Keep `capture_clipboard` as the core mobile capture command.
2. Keep triggering it from app open/resume in `src/routes/+page.svelte`.
3. Await `capture_clipboard` before `sync_now` so the manual refresh path deterministically pushes the just-captured entry in the same sync operation.
4. Keep the existing immediate `sync_entry` call after successful insert as a fast path.
5. Add visible status text such as "Clipboard captured on open" or "No new clipboard content" if users need confidence that capture happened.
6. Add tests around duplicate content and sync ordering at the Rust storage/sync layer where possible.

## Option 2: Foreground-Only Periodic Polling

### Description

Run a timer only while the Android app is visible or resumed. Every N seconds, call `capture_clipboard`, then push/pull sync. This does not provide background capture, but it improves behavior while the user keeps Copywraith open.

### Advantages

- Complies with Android clipboard restrictions because the app is foregrounded.
- Easy to implement in Svelte or Rust without a native Android service.
- Makes the Android app feel more like the desktop app while it is open.
- Can reuse `capture_clipboard`, `sync_now`, and the existing deduplication logic.

### Drawbacks

- Does not solve background capture.
- Polling can produce Android clipboard access indicators and feel noisy if the interval is too short.
- Polling wastes some battery compared with an event listener.
- Needs lifecycle cleanup to avoid timers running after the UI is hidden.

### Implementation Steps

1. Add a mobile-only foreground timer in `src/routes/+page.svelte` or in Rust lifecycle handling.
2. Start the timer when platform is Android and the window/app is focused or resumed.
3. Stop the timer when focus is lost, the page is destroyed, or the app is paused.
4. Use a conservative interval such as 30 to 120 seconds, not 5 seconds.
5. Call `capture_clipboard` first and then `sync_now`, or call only `capture_clipboard` and rely on `sync_entry` plus the existing sync loop.
6. Debounce overlapping runs with the existing `mobileRefreshInFlight` pattern.
7. Add a setting to disable foreground polling if users dislike clipboard access indicators.

## Option 3: Android Clipboard Listener While Foreground

### Description

Use Android's `ClipboardManager.OnPrimaryClipChangedListener` from native Android code or a custom Tauri plugin while Copywraith is active. When the listener fires and the app is allowed to read the clipboard, capture the new content and sync it.

This is event-driven foreground capture, not background capture.

### Advantages

- More efficient and responsive than polling.
- Avoids repeated clipboard reads when nothing changes.
- Better matches the desktop event-driven monitor model.
- Can be extended later for richer Android clipboard payloads if native code reads MIME types and URIs.

### Drawbacks

- Requires native Android/Tauri plugin work.
- Still cannot read clipboard contents after the app is backgrounded on Android 10+.
- Must manage listener registration across Android lifecycle events.
- Tauri mobile generated code is not currently checked into this repository, so this would add Android-specific project maintenance.

### Implementation Steps

1. Generate or update the Android project under `src-tauri/gen/android` with Tauri.
2. Add a small Kotlin plugin or native module that registers `ClipboardManager.OnPrimaryClipChangedListener` when the Tauri activity is resumed.
3. Unregister the listener when the activity is paused or destroyed.
4. On callback, read `primaryClip` only when the activity is resumed/focused.
5. Pass captured text to Rust through a Tauri command, plugin event, or a dedicated command that inserts supplied clipboard text.
6. Reuse `LocalStorage::insert_entry`, content hashing, and `sync_entry` so behavior stays consistent with `capture_clipboard`.
7. Version-gate and test on Android 9, 10, 12, 13, and 14 if older device support matters.

## Option 4: WorkManager Periodic Sync of Already Captured Entries

### Description

Use Android WorkManager to periodically push local entries that were already captured and pull new entries from the server. This can improve server synchronization, but it cannot capture background clipboard contents on modern Android.

This option is about durable background network sync, not clipboard monitoring.

### Advantages

- Android-supported API for deferrable background work.
- Survives app restarts better than an in-process Tauri loop.
- Respects Doze, battery saver, and connectivity constraints.
- Useful if Android should receive clipboard history from the Mac/server without opening the app.

### Drawbacks

- PeriodicWorkRequest has a minimum interval of about 15 minutes and is not exact.
- Does not read the clipboard in the background.
- Requires native Android implementation; a Worker cannot simply invoke a Svelte frontend command.
- Accessing the existing Rust/Tauri SQLite storage from a Kotlin worker needs careful design.
- Duplicating sync logic in Kotlin risks drift from `src-tauri/src/sync.rs`.

### Implementation Steps

1. Decide whether the Worker only pulls server metadata or also pushes local unsynced entries.
2. Persist sync configuration in a place native Android can read safely, such as SharedPreferences, or expose a small native bridge that reads the same settings.
3. Choose the data access strategy: call into a shared Rust library through JNI, duplicate minimal SQLite queries in Kotlin, or keep WorkManager as a wakeup that schedules work for the next app launch.
4. Add a Kotlin `CoroutineWorker` with network constraints and exponential backoff.
5. Schedule a `PeriodicWorkRequest` after settings are configured, with an interval no shorter than WorkManager allows.
6. Ensure the Worker uses the same API auth model as `SyncClient`, including primary and fallback URLs.
7. Keep conflict handling aligned with content-hash deduplication and `last_seen_server_id` cursor behavior.
8. Test with Doze, battery saver, offline network, VPN-only server access, and app force-stop.

## Option 5: Foreground Service Clipboard Monitor

### Description

Run an Android foreground service with a persistent notification and attempt to monitor the clipboard from that service.

This may keep the app process alive, but on Android 10+ it still does not grant a normal app background clipboard read access. It may only help on older Android versions or for work that does not require reading the clipboard.

### Advantages

- User-visible and more durable than an invisible background loop.
- Can run continuous network sync for already captured entries while the service is active.
- May enable clipboard monitoring on older Android versions before the clipboard restrictions were introduced.

### Drawbacks

- Not a reliable clipboard capture solution on modern Android.
- Requires persistent notification, foreground service permissions, and careful lifecycle handling.
- Has battery cost and can annoy users.
- Android 14+ foreground service type requirements and Play policy review can make this hard to justify.
- Still needs native Android code and does not naturally integrate with the Tauri runtime when the UI is gone.

### Implementation Steps

1. Treat this as an optional legacy or power-user mode, not the default strategy.
2. Add an Android foreground service with an explicit persistent notification.
3. Request notification permission on Android 13+ if notifications are needed.
4. Register `OnPrimaryClipChangedListener` inside the service only for Android versions where testing proves clipboard reads work.
5. On Android 10+, show a clear fallback message that background clipboard capture is restricted.
6. Use the service for push/pull sync of already captured entries if that justifies the persistent notification.
7. Add settings for enabling/disabling the service and choosing whether it starts at boot.
8. Test battery behavior and Play policy acceptability before shipping broadly.

## Option 6: Default Keyboard / IME Companion

### Description

Implement Copywraith as, or alongside, an Android input method editor. Android allows the current default IME to access clipboard contents in contexts where generic background apps cannot. The keyboard can also provide a paste-history UI directly above the keyboard.

This is the most realistic way to get closer to automatic clipboard access while the user is working in other apps, but it changes the product significantly.

### Advantages

- Uses a platform-recognized clipboard access exception.
- Can capture while the user is typing in other apps and the keyboard is active.
- Can provide a strong mobile UX for paste history and quick insertion.
- Avoids pretending that a normal background service can monitor the clipboard.

### Drawbacks

- Very large implementation compared with the current Tauri mobile app.
- Users must enable and trust Copywraith as a keyboard, which is a high-friction security decision.
- Keyboard apps are sensitive because they can observe typed text.
- Play policy, privacy disclosures, and onboarding need serious attention.
- IME access is still not the same as unrestricted always-on background clipboard polling.

### Implementation Steps

1. Decide whether to build a minimal companion keyboard or a full keyboard experience.
2. Add an Android IME service in Kotlin with the required manifest entries and settings activity.
3. Build onboarding that explains exactly what the keyboard can access and why.
4. When the IME is active, read clipboard content through Android `ClipboardManager` and insert it through the same local storage/sync path.
5. Expose Copywraith history inside the keyboard UI for quick paste.
6. Keep typed-text handling minimal and document what is never stored.
7. Add privacy policy updates and in-app controls to pause capture.
8. Test across major keyboards/input scenarios, password fields, work profiles, and multi-window mode.

## Option 7: Share Target, Quick Settings Tile, or Notification Action

### Description

Add explicit user-triggered capture surfaces instead of background polling.

Examples:

- Android share target: the user selects text, image, or a file in another app and shares it to Copywraith.
- Quick settings tile: the user taps "Capture Clipboard" and Copywraith opens a small foreground activity to read the clipboard.
- Notification action: a persistent or occasional notification launches a capture activity.
- In-app button: the user manually captures the current clipboard from the Copywraith UI.

### Advantages

- Compliant with Android restrictions because the user explicitly invokes Copywraith or shares content directly.
- More reliable than background polling for non-text content when using the share sheet.
- Can capture content before Android auto-clears it if the user acts promptly.
- Lower privacy risk than silent monitoring.
- Good fit for a local-first tool with power-user workflows.

### Drawbacks

- Not automatic.
- Requires user habit changes.
- Quick settings and notification actions usually still need to bring an activity foreground before clipboard reads are reliable.
- Share target implementation is native Android work and may require handling many MIME types.

### Implementation Steps

1. Add an Android share target activity or intent filter for `text/plain`, `text/html`, images, and file URIs as needed.
2. Convert shared content into Copywraith `ClipboardFlavors` and blob storage entries.
3. Add a quick settings tile that launches Copywraith into a capture-only activity or brings the Tauri activity to foreground and calls `capture_clipboard`.
4. Add an optional notification action if a persistent notification is acceptable.
5. Reuse existing content hashing and sync paths after insertion.
6. Add user-facing status feedback after capture succeeds, duplicates, or fails.
7. Test with common apps such as Chrome, Gmail, Google Photos, Files, Slack, Signal, and password managers.

### Current Implementation Progress

- Added a source-controlled `copywraith-share-target` Tauri Android plugin under `crates/copywraith-share-target`.
- The plugin contributes Android `ACTION_SEND` and `ACTION_SEND_MULTIPLE` intent filters for `text/plain`, `text/html`, `image/*`, and general `*/*` shares.
- The plugin persists incoming `EXTRA_TEXT` and `EXTRA_STREAM` payloads into Copywraith's Android app data directory under `pending-shares` so transient URI grants are consumed while the share intent is active.
- The plugin processes the launch intent on load and `onNewIntent`; the Rust importer moves processed JSON batches aside so the same staged batch is not imported twice.
- Follow-up fix: `import_pending_shares` now asks the native plugin to collect the Activity's current intent immediately before scanning `pending-shares`. This covers warm-start share flows where Copywraith is already alive and Svelte would otherwise run before the native plugin had staged the share payload.
- `src-tauri/src/commands.rs` now exposes `import_pending_shares`, which imports staged share batches into local storage, deduplicates through the existing content-hash path, emits UI refresh events, and triggers server sync for new entries.
- `src/routes/+page.svelte` now checks `has_pending_shares`, imports staged Android shares before clipboard capture and `sync_now`, and only shows the System 7 modal when a share-sheet payload is actually waiting.
- Normal mobile open/resume refreshes use compact footer progress for clipboard capture, server sync, and list reload instead of blocking the UI with a modal.
- The Android list is reloaded immediately after a shared item imports locally, before waiting for server upload/pull, so the user sees the new entry as soon as it is stored on-device.
- The Android list is also reloaded immediately when lifecycle clipboard capture stores a new local item on open/resume, before waiting for server sync.
- The mobile footer keeps a three-column layout of item count, compact progress bar, and sync endpoint status so `Sync: checking...` stays on the right instead of wrapping under the item count.
- The frontend sync status watchdog now falls back to the last stable online/disabled endpoint status after a stale checking timeout, reducing false persistent `Sync: unreachable` states when no sync is actively running.
- Shared images are stored as Copywraith image entries when the MIME type starts with `image/` or the bytes are recognized as an image.
- Shared non-image files are stored as file entries with the file bytes in blob storage and the display name in `file_list`.
- The Android plugin caps each staged shared file at 64 MiB to avoid unbounded app-private storage growth.
- Follow-up sync fix: Android pull sync now downloads blob data for blob-backed file entries as well as images, and it no longer advances the persisted pull cursor when any remote entry fails to import.
- Added a mobile Settings repair action, "Reset Sync Cursor", to force the Android client to re-scan server entries without deleting local data when the phone list drifts from the web UI.

Current limitations:

- Android share import is wired for Android only; iOS share extensions are not implemented.
- `text/html` shares are currently imported as text because many Android apps provide HTML through `EXTRA_TEXT`; richer HTML flavor extraction can be added after device testing.
- File entries preserve the shared file's display name and bytes but do not preserve the original content URI after import.
- The generated `src-tauri/gen/android` tree remains ignored; the durable implementation is the source-controlled Tauri plugin, not manual edits to generated Android files.
- This still requires Android device/emulator testing with real share intents.

Troubleshooting notes:

- If sharing to Copywraith opens the app but creates no entry, check Android logs for `CopywraithSharePlugin` staging errors and verify `pending-shares` JSON batches are created in app data.
- If the Android list differs from the web UI after intermittent connectivity, use Settings -> Reset Sync Cursor on Android, then run Sync Now or reopen the app.
- Intermittent "sync unreachable" can still be transient if both configured endpoints fail during an active sync. The endpoint status should be read with the role/URL shown in the status text.

## Option 8: Accessibility Service

### Description

Use an Android accessibility service to observe UI events or assist with paste/capture workflows.

This is not a good solution for clipboard sync. Accessibility services do not provide a clean, general clipboard-change API, and using accessibility for background data capture is risky from a privacy and policy perspective.

### Advantages

- Can observe some app/window events that normal apps cannot.
- Can support explicit assistive workflows if Copywraith later needs accessibility features.

### Drawbacks

- Does not directly solve background clipboard reads on modern Android.
- High user trust burden because accessibility permissions are broad.
- High Play Store policy risk if the core use case is not accessibility.
- Can be brittle across apps and Android versions.
- Easy to overreach into screen/text observation that Copywraith should avoid.

### Implementation Steps

1. Avoid this option for clipboard sync unless there is a separate genuine accessibility feature.
2. If pursued, define the narrow accessibility use case and document it in product/privacy text.
3. Implement an AccessibilityService with minimal event subscriptions.
4. Do not treat accessibility events as clipboard contents unless the user explicitly invokes capture.
5. Add prominent controls to disable the service and delete captured data.
6. Verify Play policy before investing implementation time.

## Option 9: BroadcastReceiver and IntentService

### Description

Create a `BroadcastReceiver` for clipboard changes and an `IntentService` to sync those changes.

This was suggested in the original sketch, but it is not a viable Android clipboard architecture.

### Advantages

- Familiar Android pattern for some system events.
- Simple in theory.

### Drawbacks

- Android does not send a public clipboard-changed broadcast to third-party apps.
- `IntentService` is deprecated.
- Even if a receiver wakes the app for another reason, modern Android still blocks background clipboard reads.
- Adds native complexity without solving the actual problem.

### Implementation Steps

1. Do not implement this for clipboard monitoring.
2. Use WorkManager for scheduled background network sync if background work is needed.
3. Use a foreground Activity, IME, or explicit share/capture action for clipboard capture.

## Option 10: Shizuku / Sui Privileged Helper

### Description

Use Shizuku to let Copywraith run a small native helper with ADB shell or root identity. Shizuku starts a privileged process through ADB or root and exposes it to normal apps through a Binder API. Sui is the Magisk/root variant that provides similar integration for rooted devices.

For clipboard sync, the helper would read the clipboard through Android system APIs as `shell` or `root`, then either upload changes directly to the Copywraith server or notify the main app when it is alive. This is different from a normal background service because clipboard access checks see the Shizuku helper's privileged identity, not the normal Copywraith app UID.

On AOSP, the shell package includes `android.permission.READ_CLIPBOARD_IN_BACKGROUND`, so Shizuku in ADB mode is a plausible way to read clipboard contents while Copywraith itself is backgrounded. Root/Sui can be stronger. This still needs runtime validation on target Android versions and OEM builds.

### Advantages

- Best practical route for power users who want real background Android clipboard capture without building a custom ROM.
- Can avoid Android's normal foreground/default-IME clipboard restriction if the Shizuku backend has the needed permission.
- More accessible than full root for Android 11+ users because wireless debugging can start Shizuku on-device.
- A Shizuku `UserService` can run code with shell/root identity instead of scraping command output from `adb shell`.
- Can be implemented as an explicit advanced mode without changing the baseline Android app behavior.
- A listener-based implementation can capture changes quickly without aggressive polling.

### Drawbacks

- Requires users to install and trust Shizuku, grant Copywraith Shizuku permission, and start Shizuku after reboot in non-root mode.
- Non-root Shizuku runs as ADB shell, not full root. Shell permissions vary by Android version and OEM.
- Uses privileged/hidden system APIs such as `IClipboard`; method signatures and access behavior can change across Android releases.
- Shizuku `UserService` is not a normal Android app process. APIs such as `Context#registerReceiver` and `ContentResolver` may not behave like they do in an Activity/service process.
- The helper cannot normally write into Copywraith's app-private SQLite database when running as shell. Shell cannot read/write `/data/user/0/<copywraith-package>` like the app UID can.
- Direct server upload means the Shizuku helper needs access to server URL and password/API credential, which increases the sensitivity of the integration.
- This bypasses Android's clipboard privacy model, so the feature needs clear UI, auditability, and an easy off switch.
- Not suitable as a mainstream Play Store default feature.

### Architecture Choices

#### Direct-To-Server Helper

The Shizuku helper reads clipboard changes, hashes/deduplicates them, and uploads directly to `/api/entries`.

Advantages:

- Works even when the Tauri UI/process is not alive.
- Avoids writing to app-private SQLite from the wrong UID.
- Keeps the background path relatively self-contained.

Drawbacks:

- Requires sharing server URL and password with the Shizuku helper.
- Local Android cache will not see entries until the normal app later pulls from the server.
- Duplicates some sync/request logic outside Rust `SyncClient` unless a shared native library is introduced.

Implementation steps:

1. Store a minimal Shizuku sync configuration in a place the normal app can pass to the helper intentionally.
2. Include primary URL, fallback URL, and auth password/API field only if the user enables Shizuku mode.
3. Implement content hashing compatible with `copywraith-core`.
4. POST new text entries to `/api/entries` using the same `CreateEntryRequest` shape.
5. Let the normal Android app pull those entries later through existing sync.

#### Helper-To-App IPC

The Shizuku helper reads clipboard changes and calls back into Copywraith's normal app process to insert entries locally and sync them.

Advantages:

- Reuses existing local storage and Rust sync behavior.
- Keeps server credentials inside the normal app process if IPC only passes clipboard content.
- Android UI updates can happen immediately when the app is alive.

Drawbacks:

- Does not help when the normal app process is killed unless the helper stores a pending queue elsewhere.
- Requires a bound service, content provider, or other IPC endpoint exposed by Copywraith.
- IPC endpoints must be permission-protected so other apps cannot inject clipboard entries.

Implementation steps:

1. Add a native Android service or provider in Copywraith that accepts clipboard text from the Shizuku helper.
2. Protect it with a signature-level, package-private, or explicit caller-verification mechanism as far as possible.
3. Forward received content to Rust insertion logic or a dedicated Tauri command when the runtime is available.
4. Queue entries safely if the frontend is not loaded but the app process is alive.
5. Fall back to direct-to-server or no-op if the app process is unavailable.

#### Shizuku Poller

The helper wakes on an interval, reads the clipboard, compares the content hash with the last uploaded value, and uploads changes.

Advantages:

- Simpler than hidden listener APIs.
- Easier to debug and version-gate.
- May be enough if a 30 to 120 second capture delay is acceptable.

Drawbacks:

- Repeated clipboard reads may trigger access indicators or privacy concerns depending on OS behavior.
- Polling wastes more battery than event-driven capture.
- May still miss clipboard contents that are copied and replaced between polling intervals.

Implementation steps:

1. Implement a Shizuku `UserService` with a conservative timer.
2. Read the current clipboard through system clipboard APIs as shell/root.
3. Hash text content and compare with the last seen hash.
4. Upload only changed, non-empty content.
5. Expose settings for interval, Wi-Fi/VPN-only sync, and pause/resume.

#### Shizuku Clipboard Listener

The helper registers a clipboard change listener through system clipboard Binder APIs and uploads when notified.

Advantages:

- More responsive and efficient than polling.
- Less likely to miss short-lived clipboard contents.
- Closest Android equivalent to the desktop clipboard monitor for power users.

Drawbacks:

- Requires hidden/internal APIs such as `IClipboard` and `IOnPrimaryClipChangedListener`.
- API shape changes across Android versions and may need reflection, generated stubs, or HiddenApiBypass-style tooling.
- Needs careful lifecycle handling when Shizuku dies, restarts, or loses permission.

Implementation steps:

1. Add Shizuku API and provider dependencies to the Android project.
2. Request Shizuku permission from the normal app UI.
3. Check `Shizuku.getUid()` and show whether the backend is `shell` (`2000`) or `root` (`0`).
4. Bind a Shizuku `UserService` for clipboard monitoring.
5. In the service, acquire the clipboard system service Binder and register a primary-clip listener for the active user.
6. On callback, read clipboard content as shell/root, hash it, and upload or IPC it to Copywraith.
7. Re-register the listener when Shizuku restarts and stop cleanly when the user disables the feature.
8. Runtime-test the feature at setup by attempting a background clipboard read and reporting success/failure.

### Recommended Shizuku Design

For Copywraith, the most practical Shizuku path is:

1. Add an explicit "Advanced: Shizuku background clipboard sync" setting.
2. Require the user to install/start Shizuku and grant Copywraith permission.
3. Verify backend UID and clipboard read capability before enabling.
4. Prefer a Shizuku clipboard listener; use polling only as a fallback.
5. Upload directly to the server from the helper, then let the Android app pull those entries into its local cache later.
6. Keep lifecycle/foreground capture as the default path for all users.
7. Show persistent status for whether Shizuku sync is active, stopped, missing permission, or missing backend.
8. Never silently enable Shizuku clipboard monitoring.

### Current Implementation Progress

- Added Shizuku API/provider dependencies to the source-controlled Android plugin under `crates/copywraith-share-target`.
- Added an optional Settings control, `Advanced Android Clipboard`, that requests Shizuku permission and enables/disables the listener.
- Added a Shizuku `UserService` (`ShizukuClipboardService`) that runs as Shizuku/Sui, gets the system clipboard Binder, and registers a primary-clip listener through Binder transactions.
- The listener captures text changes, deduplicates the most recent text, stages a local pending-share batch for the main app, and triggers the existing pending-share import path when the app is alive.
- The listener also tries direct server upload to the configured primary URL, then fallback URL, using the same password/API field as normal sync.
- Devices without Shizuku, without a running Shizuku backend, with denied permission, or with incompatible Shizuku versions report a benign status and fall back to normal Android open/resume capture plus share-sheet import.
- The listener is opt-in only and starts on app launch only if `shizuku_clipboard_enabled` is already saved in local settings.
- Current implementation supports text payloads only; images/files should still use Android share-sheet import.

### Testing Checklist

- Shizuku ADB mode on Android 11+ with wireless debugging.
- Shizuku ADB mode after reboot before and after Shizuku is restarted.
- Sui/root mode on a rooted test device.
- Android 10, 12, 13, 14, and 15 if possible.
- At least one non-Pixel OEM build because shell clipboard permissions can vary.
- Device locked/unlocked, screen off/on, app backgrounded, app force-stopped, and Shizuku service killed.
- Clipboard access indicators and user-visible privacy behavior.
- Server unavailable, VPN unavailable, primary URL unavailable, and fallback URL available.
- Password changes on the Copywraith server while Shizuku helper has stale credentials.
- Rapid clipboard changes and clipboard auto-clear behavior.

## Option 11: Device-Owner, Privileged, System App, or Root Deployment

### Description

For managed devices, rooted devices, or system-image deployments, Copywraith could be granted privileges unavailable to normal apps. That can make true background clipboard monitoring possible in controlled environments.

### Advantages

- Can provide the closest behavior to desktop-style continuous clipboard sync.
- Useful for personal rooted devices, kiosks, enterprise device-owner deployments, or custom ROMs.
- Can run native background services without ordinary consumer-app limitations if the deployment grants the right privileges.

### Drawbacks

- Not viable for standard Play Store distribution.
- Not viable for most users.
- Higher security risk because clipboard contents are sensitive.
- Requires separate documentation, build flavors, and support expectations.
- Device-owner APIs alone may not grant every clipboard privilege; exact capability depends on deployment model and Android version.

### Implementation Steps

1. Define this as a separate advanced deployment mode, not the main Android app behavior.
2. Decide the target environment: rooted personal device, enterprise device owner, custom ROM, or system app.
3. Add a separate Android build flavor and manifest permissions as appropriate for that environment.
4. Implement a native foreground or background service with `ClipboardManager` listener/polling.
5. Reuse Copywraith hashing, storage, and sync semantics.
6. Add explicit warnings about sensitive clipboard data and unsupported consumer devices.
7. Test on the exact target OS image because privilege behavior varies by Android release and OEM.

## Option 12: Server Push or FCM Wakeup for Remote Pull

### Description

Use Firebase Cloud Messaging or another push mechanism to tell Android that the server has new entries, then pull them into the local cache. This helps Android stay current with Mac/server history, but it does not capture Android clipboard changes in the background.

### Advantages

- Better battery behavior than frequent polling for remote updates.
- Useful when the Mac app uploads clipboard entries and Android should receive them quickly.
- Can coexist with lifecycle capture and explicit Android capture actions.

### Drawbacks

- Does not read the Android clipboard.
- Requires push infrastructure, device registration, and server changes.
- Local-network-only or VPN-only deployments may not fit FCM well.
- Android may still defer background work depending on priority, Doze, and app standby state.

### Implementation Steps

1. Decide whether Copywraith's local-first/private-network posture is compatible with FCM or another push channel.
2. Add device registration to the Android app and store device tokens server-side.
3. Send a push when the server stores a new entry from another device.
4. On push receipt, run a constrained background pull if allowed, or show a notification that opens Copywraith and syncs.
5. Keep auth behavior aligned with password-protected server access.
6. Ensure notification content does not expose clipboard data unless explicitly enabled.

## Recommended Path

Recommended short-term plan:

1. Keep lifecycle capture as the supported Android behavior.
2. Make mobile refresh ordering deterministic by capturing before manual `sync_now`, or document that immediate `sync_entry` handles new entries.
3. Add foreground-only polling or a foreground clipboard listener if users keep the app open and expect it to update live.
4. Add an explicit capture button and status message so users can force capture/sync with confidence.
5. Add a share target for text first, then consider images/files if Android mobile blob capture becomes a priority.

Recommended medium-term plan:

1. Add WorkManager only if Android needs durable background push/pull of already captured entries.
2. Consider an IME companion only if mobile paste-history UX becomes a major product goal.
3. Consider Shizuku/Sui as the explicit advanced-user path for real background clipboard capture.
4. Avoid foreground-service clipboard monitoring unless targeting older Android versions or an explicit power-user mode.
5. Avoid AccessibilityService, BroadcastReceiver, and IntentService for clipboard sync.

## Testing Checklist

Test any Android clipboard-sync change across these cases:

- Android 9 or older if pre-Android-10 behavior is intentionally supported.
- Android 10 or newer to verify background clipboard restrictions.
- Android 12 or newer to observe clipboard access indicators.
- Android 13 or newer to account for notification permission and clipboard auto-clear behavior.
- Shizuku ADB mode and Sui/root mode if privileged sync is implemented.
- App foreground, app backgrounded, app force-stopped, device locked, and device after reboot.
- Online, offline, VPN-only server, primary URL unavailable, and fallback URL available.
- Duplicate clipboard content, empty clipboard content, rapidly changing clipboard content, and sensitive content from password managers.
- Battery saver, Doze, and restricted app battery mode.

## Bottom Line

Periodic background clipboard capture is not a reliable option for a normal Android Copywraith app on modern Android. Periodic background sync of entries that Copywraith has already captured is possible, but it requires native Android background work and does not solve clipboard monitoring. Shizuku/Sui can improve the situation for power users by running a helper as ADB shell or root, and should be the preferred advanced route if true background capture is required. The mainstream Android design should still combine foreground/lifecycle capture, explicit user-triggered capture, and optional native enhancements rather than trying to recreate the Mac desktop clipboard monitor in the background.
