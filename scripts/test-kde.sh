#!/usr/bin/env bash
# Compile the production D-Bus adapters without the unrelated Tauri/WebKit stack.
set -euo pipefail
repo_dir=$(cd "$(dirname "$0")/.." && pwd)
harness_dir=$(mktemp -d)
trap 'rm -rf "$harness_dir"' EXIT
mkdir -p "$harness_dir/src"
cat > "$harness_dir/Cargo.toml" <<'MANIFEST'
[package]
name = "copywraith-kde-tests"
version = "0.0.0"
edition = "2021"
[dependencies]
dbus = "0.9"
MANIFEST
cat > "$harness_dir/src/lib.rs" <<RUST
#[path = "$repo_dir/src-tauri/src/linux/shortcuts/kde.rs"]
mod kde;
#[path = "$repo_dir/src-tauri/src/linux/notifications.rs"]
mod notifications;
RUST
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$repo_dir/target/kde-tests"}
if [[ ${COPYWRAITH_KDE_TEST_ISOLATED:-} == 1 ]]; then
    cargo test --manifest-path "$harness_dir/Cargo.toml" plasma_runtime -- --ignored --test-threads=1 --nocapture
    exit
fi
export XDG_CONFIG_HOME="$harness_dir/config" XDG_DATA_HOME="$harness_dir/data"
export XDG_CACHE_HOME="$harness_dir/cache" XDG_RUNTIME_DIR="$harness_dir/runtime"
mkdir -m 700 -p "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" "$XDG_RUNTIME_DIR"
dbus-run-session --config-file "$repo_dir/scripts/kde-session.conf" -- cargo test --manifest-path "$harness_dir/Cargo.toml" -- --include-ignored --skip plasma_runtime --test-threads=1
