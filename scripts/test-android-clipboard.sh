#!/usr/bin/env bash
set -euo pipefail

# This test changes clipboard/adb state only inside a disposable emulator.
: "${COPYWRAITH_TEST_ANDROID_SERIAL:?Set the task-owned emulator serial}"
serial=$COPYWRAITH_TEST_ANDROID_SERIAL
apk=${1:?Pass the debug application APK}
case "$serial" in
  emulator-*) ;;
  *) echo "Refusing a non-emulator target" >&2; exit 1 ;;
esac
[[ -f "$apk" ]] || { echo "Debug APK not found" >&2; exit 1; }

adb=("${ADB:-adb}" -s "$serial")
[[ "$("${adb[@]}" shell getprop ro.kernel.qemu | tr -d '\r')" == 1 ]] || {
  echo "Target is not an Android emulator" >&2
  exit 1
}

readonly probe_class=ch.lkmc.copywraith.share.ClipboardIdentityProbe
readonly remote_apk="/data/local/tmp/copywraith-clipboard-probe-$$.apk"
readonly remote_pid="/data/local/tmp/copywraith-clipboard-probe-$$.pid"
readonly probe_timeout=30s
readonly shell_uid=2000
readonly root_uid=0

cleanup() {
  # A timed-out probe may outlive the adb client. Kill only our recorded process.
  "${adb[@]}" shell "
    if [ -f '$remote_pid' ]; then
      pid=\$(cat '$remote_pid')
      case \"\$pid\" in ''|*[!0-9]*) ;; *)
        if tr '\000' ' ' < /proc/\$pid/cmdline 2>/dev/null | grep -F -q '$probe_class'; then
          kill \"\$pid\" 2>/dev/null || true
        fi ;;
      esac
    fi
    rm -f '$remote_pid' '$remote_apk'
  " >/dev/null 2>&1 || true
  "${adb[@]}" unroot >/dev/null 2>&1 || true
}
trap cleanup EXIT

"${adb[@]}" push "$apk" "$remote_apk" >/dev/null
"${adb[@]}" shell chmod 644 "$remote_apk"

for mode in unroot root; do
  "${adb[@]}" "$mode"
  "${adb[@]}" wait-for-device
  expected_uid=$shell_uid
  [[ "$mode" != root ]] || expected_uid=$root_uid
  actual_uid=$("${adb[@]}" shell id -u | tr -d '\r')
  [[ "$actual_uid" == "$expected_uid" ]] || {
    echo "Emulator does not support the requested adb mode" >&2
    exit 1
  }

  timeout "$probe_timeout" "${adb[@]}" shell "
    echo \$\$ > '$remote_pid'
    export CLASSPATH='$remote_apk'
    exec app_process /system/bin '$probe_class' --isolated-emulator
  "
done
