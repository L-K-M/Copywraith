# Privileged clipboard boundary

The app owns storage, credentials and sync. Shizuku observes; service version 2
removes its legacy HTTP uploader. Activity staging still needs durable ingress
integration. This is not yet full background synchronization.

## Framework ABI

`ClipboardDriver` uses the published AOSP `IClipboard.aidl` declaration order:

| Android API | Read / add / remove | Identity arguments |
| --- | --- | --- |
| 24–27 | 2 / 5 / 6 | package; removal has none |
| 28 | 3 / 6 / 7 | package; removal has none |
| 29–30 | 3 / 6 / 7 | package, user |
| 31–33 | 4 / 7 / 8 | package, user |
| 34–36 | 4 / 7 / 8 | package, attribution, user, device |

Transactions start at `IBinder.FIRST_CALL_TRANSACTION`. Listener arguments precede
identity arguments. Unknown future versions fail rather than guess.

Primary source: [AOSP IClipboard.aidl](https://android.googlesource.com/platform/frameworks/base/+/android-16.0.0_r1/core/java/android/content/IClipboard.aidl),
checked against `android-{7,8,9,10,11,12,13,14,15,16}.0.0_r1`.
`clearPrimaryClip` shifts the table in API 28; `setPrimaryClipAsPackage` shifts it
again in API 31. API 31 does **not** take an attribution tag.

## Identity

Use `com.android.shell`, which has background clipboard permission. A root UID
with that package is insufficient: `addActiveOwnerLocked` also checks package
ownership, including for text. The dedicated service initializes its process
leader as shell before publishing its Binder. This changes no Shizuku daemon or
host credentials. It is not a claim that every pre-existing thread loses root.

Older Binder kernels use the process leader's UID; newer kernels use the sending
thread's effective UID. Initialize the leader, then prepare every IPC sender,
including pre-existing Binder workers. Startup outside that leader fails closed.
The calling app's UID selects the Android user; API 24–28 cannot select another
user through this interface. Virtual-device clipboard selection is not implemented.

Primary sources:
- [ClipboardService ownership and access checks](https://android.googlesource.com/platform/frameworks/base/+/android-14.0.0_r1/services/core/java/com/android/server/clipboard/ClipboardService.java).
- Binder sender identity: [Android 14/6.1](https://android.googlesource.com/kernel/common/+/refs/heads/android14-6.1/drivers/android/binder.c) uses `task_euid(proc->tsk)`;
  [Android 16/6.12](https://android.googlesource.com/kernel/common/+/refs/heads/android16-6.12/drivers/android/binder.c) uses `current_euid()`.
- [Shizuku main-thread construction, then Binder handoff](https://github.com/RikkaApps/Shizuku/blob/master/starter/src/main/java/moe/shizuku/starter/ServiceStarter.java).
- [Shizuku service construction](https://github.com/RikkaApps/Shizuku-API/blob/master/server-shared/src/main/java/rikka/shizuku/server/UserService.java).

## Evidence limits

Three old marshalling regressions failed before extraction. Eight Robolectric
tests cover layouts, rich payload decoding, ambiguous registration, failed cleanup
and per-call sender preparation. Preparing only once fails the sender-context
mutation test. These model contracts, not OS credential changes or every release.

`scripts/test-android-clipboard.sh <debug-apk>` exercises actual root/shell Binder
reads and callbacks in a disposable emulator. It requires an explicit
`COPYWRAITH_TEST_ANDROID_SERIAL` and refuses non-emulator targets. The debug-only
probe compiles, but has not run here.

Root/shell startup, SELinux, package attribution, actual callbacks and secondary
users still require isolated device tests. JVM tests establish none of those OS
guarantees. Foreground-service lifecycle and durable capture identity are separate
gates. No clipboard contents or server credentials belong in logs or notifications.
