#!/usr/bin/env bash
# Never run the integration test on the caller's display, bus or KDE config.
set -euo pipefail
repo_dir=$(cd "$(dirname "$0")/.." && pwd)
if [[ ${1:-} != --inside-session ]]; then
    session_dir=$(mktemp -d)
    trap 'rm -rf "$session_dir"' EXIT
    export XDG_CONFIG_HOME="$session_dir/config"
    export XDG_DATA_HOME="$session_dir/data"
    export XDG_CACHE_HOME="$session_dir/cache"
    export XDG_RUNTIME_DIR="$session_dir/runtime"
    mkdir -m 700 -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR"
    export COPYWRAITH_KDE_TEST_ISOLATED=1
    exec_args=(dbus-run-session --config-file "$repo_dir/scripts/kde-session.conf" -- "$0" --inside-session)
    "${exec_args[@]}"
    exit
fi
[[ ${COPYWRAITH_KDE_TEST_ISOLATED:-} == 1 ]]
# Xvfb chooses a free display; no commands can reach the host's X server.
Xvfb -displayfd 3 -screen 0 1280x800x24 3>"$XDG_RUNTIME_DIR/display" >"$XDG_RUNTIME_DIR/xvfb.log" 2>&1 &
xvfb_pid=$!
trap 'kill "$xvfb_pid" 2>/dev/null || true; wait "$xvfb_pid" 2>/dev/null || true' EXIT
for attempt in {1..100}; do
    [[ -s "$XDG_RUNTIME_DIR/display" ]] && break
    if ! kill -0 "$xvfb_pid" 2>/dev/null; then
        cat "$XDG_RUNTIME_DIR/xvfb.log" >&2
        exit 1
    fi
    sleep 0.1
done
if [[ ! -s "$XDG_RUNTIME_DIR/display" ]]; then
    echo 'Xvfb did not publish a display before the startup deadline' >&2
    cat "$XDG_RUNTIME_DIR/xvfb.log" >&2
    exit 1
fi
export DISPLAY=":$(cat "$XDG_RUNTIME_DIR/display")"
unset WAYLAND_DISPLAY
export XDG_CURRENT_DESKTOP=KDE XDG_SESSION_TYPE=x11 QT_QPA_PLATFORM=xcb
# Plasma 5 and 6 package layouts differ; use only the disposable session daemon.
for candidate in /usr/libexec/kglobalacceld /usr/lib/x86_64-linux-gnu/libexec/kglobalacceld /usr/bin/kglobalaccel5; do
    if [[ -x "$candidate" ]]; then
        export COPYWRAITH_KGLOBALACCELD="$candidate"
        break
    fi
done
: "${COPYWRAITH_KGLOBALACCELD:?KGlobalAccel daemon is required}"
"$repo_dir/scripts/test-kde.sh"
