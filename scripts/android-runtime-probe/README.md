# Blocking Android lifecycle probe

Debug-only prototype, gated by `android-runtime-probe`. Release builds reject
that feature. No monitoring, capture driver, scheduler policy, or production
background-mode implementation is included. Shizuku stays disabled on the fresh
installation. Existing production behavior is unchanged outside the feature.

The test uses the real frontend, Tauri/Wry, SQLite storage, SyncClient, and
an authenticated production server fixture. The temporary `dataSync` service
holds the process lease; ordinary JobService execution owns HTTP exchange.
There is no Activity sync loop in the probe.

```text
Application ActivityLifecycleCallbacks -> native Activity-ready adapter -> Tauri
Debug service / JobService            -> native runtime -> MobileCore -> storage/sync
```

Activity-ready fires at `onActivityStarted`, after Wry's `onCreate` context
registration. Native window reconciliation is dispatched from a worker because
Tauri executes `run_on_main_thread` inline on its own event thread. Reconciliation
also follows native window destruction, covering a new Activity arriving before
the old Rust window is removed. Wry retains configuration-change attributes;
the probe checks that a recreated Activity gets a working WebView.

## Build

Use Java 17, an installed Android SDK/NDK, Node matching package.json, and Rust
1.98 with `x86_64-linux-android` already installed. Set `JAVA_HOME`, `ANDROID_HOME`,
`NDK_HOME`, and optionally the shared `CARGO_TARGET_DIR`/`GRADLE_USER_HOME`.
No defaults, SDK targets, or Rust installations are changed by these scripts.

```sh
npm ci
cargo test --locked -p copywraith-server --test mobile_core --test android_runtime
scripts/android-runtime-probe/build.sh
```

The installer overlays tracked `debug` and `androidTest` sources onto the ignored
Tauri-generated project. It retains generated SDK and compiler versions.
The build includes `:copywraith-share-target:testDebugUnitTest`; this base branch
has no parcel tests, so parent integration must supply those tests.

## Run

Use a disposable API 33+ x86_64 emulator with working WebView and no
Copywraith installation. Set `ANDROID_SERIAL=emulator-5554` explicitly (or the
selected emulator serial). The runner verifies emulator identity before mutation. It installs both APKs, forwards the
loopback fixture port, runs instrumentation, and removes its installations.

```sh
ANDROID_SERIAL=emulator-5554 node scripts/android-runtime-probe/run.mjs
```

Existing reverse mappings at tcp:18763 are refused. Cleanup removes only a
successfully reserved mapping that still targets the fixture port. Commands have
15-second timeouts; instrumentation has a five-minute timeout. Tests can override
these with `ANDROID_PROBE_COMMAND_TIMEOUT_MS` and
`ANDROID_PROBE_INSTRUMENTATION_TIMEOUT_MS`. Partial command failures and fixture
startup logs are retained without collecting unrelated device logs.

Evidence is written to `artifacts/android-runtime/`. Success requires the
instrumentation runner's `OK (1 test)`, not merely adb's exit status.

Assertions cover cold service initialization with zero Tauri startups/windows;
first functional UI using the same core/storage/client; final Activity destruction
and zero windows; an actual protocol upload and download under a job lease;
working IPC and rendered UI on reopen, Activity recreation, and three rapid
reopens; one Tauri startup/window; and lease release after service teardown.
Fixture settings are applied only after the first Activity is destroyed, so UI
startup cannot account for the HTTP proof. Host tests cancel the same job runtime
after server commit but before reply and require byte-identical frozen replay.

## Results and follow-up

G1 passed on API36 at `e1884f2` in CI `34041898579` (26.437 seconds).
That bounded same-process lifecycle result remains valid. It did not stop a
running job through Android's framework callbacks.

The follow-up test holds an operation response after server commit, delivers a
second job to the busy native reservation, and requires an OS-owned retry after
admission settles. It then uses API36's `cmd jobscheduler stop` with
`STOP_REASON_USER`, whose backoff prevents an immediate automatic restart.
`ANDROID_JOB_STOP` instrumentation markers separately report busy admission,
framework stop delivery/reference identity, and native teardown. The test checks
lease release within three seconds, before the client's 30-second HTTP timeout,
with one unsynced entry and no returned mutation response. Protocol push pulls
before freezing candidates, so download state and feed counters are compared
against their pre-stop baseline. After teardown, a quiet interval must produce
no additional work. The test forces the retained second job without rescheduling
it, requires its exact replacement token to complete, checks byte-identical
requests and cached receipts, and retains every functional reopening assertion.

API36 CI `34050426309` at `5d5b9ab` reproduced both bugs: busy admission lost the
retry, and a separately parceled stop left native work and its lease running.
The handler now matches the framework job ID and cancels the stored native token.
Busy admission returns ongoing and requests `jobFinished(parameters, true)`;
JobServiceEngine dispatches that completion after acknowledging start, leaving
retry timing to OS backoff. The probe uses only the default scheduler namespace.
The corrected native test awaits parent CI; no native green is claimed yet.
The host cancellation fixture validates the HTTP controls and replay assertions;
it cannot prove Android stop handling or scheduler retry retention.

The optional workflow runs only
by manual dispatch or a parent push to `probe/android-runtime-*`; it creates no
PR and performs no merge. Emulator setup follows the
[runner's documented setup](https://github.com/ReactiveCircus/android-emulator-runner).

The short-lived FGS is test scaffolding. Production still needs job scheduling,
quota/retry policy, missed-hint recovery, service policy, notification handling,
and the separately owned ingress/capture work. The fixture tests text protocol
exchange, not the complete deletion/star/blob matrix. The new OS stop/retry test,
timeout, sticky restart, force-stop, Doze, permission changes, and root/shell
Shizuku/SELinux behavior require additional device evidence. No alternate process
layout is selected by this prototype.

The debug invoke boundary rejects automatic `sync_now`, clipboard capture, and
share import requests. The existing frontend starts these on mount/focus; allowing
them would introduce unleased network tasks and contaminate the job-only proof.
UI evidence therefore covers rendering and real read IPC, not those three actions.

Use an isolated checkout: the generated debug overlay requires the probe feature
until that generated Android project is removed and regenerated for normal builds.

ExitRequested commits its decision under the acquisition lock. Once exit commits,
service and job acquisition return no lease; Kotlin stops the service or returns
the rejected job to scheduler backoff.
Inspection alone does not close admission. The service-held instrumentation
scenario does not exercise this final-exit race; deterministic Rust tests do.

UI checks use a new UUID per call and publish success only after that call’s IPC
replies. Expected downloaded text must appear in rendered body text. The shared
JavaScript asset is tested directly under Node, including stale-success rejection.

```sh
node --test scripts/android-runtime-probe/tests/*.test.mjs
```
