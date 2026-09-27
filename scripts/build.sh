#!/usr/bin/env bash
#
# Build every Copywraith target this host can build, staging artifacts under
# dist/ at the repo root.
#
#   scripts/build.sh                 # all feasible targets
#   scripts/build.sh frontend server # only the named targets
#   scripts/build.sh --debug         # debug builds where meaningful
#   scripts/build.sh --install       # also install what was built (see below)
#
# Targets:
#   frontend   Svelte popup UI -> build/, staged to dist/frontend/
#              (src-tauri embeds it via tauri::generate_context!, so the
#              desktop and Android builds run this first automatically)
#   server-ui  Server admin UI -> server/ui/dist/, staged to dist/server-ui/
#   server     copywraith-server binary -> dist/copywraith-server
#   desktop    Tauri bundles for this host OS -> dist/desktop/
#              Linux needs the webkit2gtk-4.1 dev packages (see README.ubuntu.md);
#              missing system deps skip the target on a default run
#   android    Universal release APK for all four ABIs -> dist/android/
#              Needs JDK 17, the Android SDK, an NDK, and the Rust android
#              targets (scripts/android-dev-bootstrap.sh installs them).
#              CW_ANDROID_TARGETS overrides the ABI list (default: all four)
#   docker     Server image -> copywraith-server:<version> (docker daemon only)
#
# --install semantics per target:
#   desktop    .deb -> sudo apt install; otherwise AppImage -> ~/Applications
#   android    adb install -r (a device or emulator must be connected)
#   server     cargo install --path server --force (lands in ~/.cargo/bin)
#   docker     docker compose up -d --build (local stack redeploy)
#   frontend, server-ui  nothing to install (reported, not an error)
#
# Missing toolchains skip a target on a default run but fail it when the
# target was named explicitly. The summary at the end lists what happened.
set -uo pipefail

cd "$(dirname "$0")/.." || exit 1
REPO_ROOT="$(pwd)"
DIST="$REPO_ROOT/dist"
export PATH="$HOME/.cargo/bin:$PATH"

if command -v rustc >/dev/null 2>&1; then
  HOST_TRIPLE=$(rustc -vV 2>/dev/null | sed -n 's/^host: //p' | tr 'a-z-' 'A-Z_')
fi
HOST_TRIPLE="${HOST_TRIPLE:-X86_64_UNKNOWN_LINUX_GNU}"

INSTALL=0
DEBUG=0
NAMED=()

say() { printf '%s\n' "$*"; }
step() { say "==> $*"; }
note() { say "-- $*"; }
err() { say "!! $*" >&2; }
skip() { say ".. $*"; }

for arg in "$@"; do
  case "$arg" in
    --install) INSTALL=1 ;;
    --debug) DEBUG=1 ;;
    --help|-h) awk 'NR==1 && /^#!/ {next} /^#/ {sub(/^# ?/,""); print; next} {exit}' "$0"; exit 0 ;;
    --*) err "unknown flag: $arg"; exit 2 ;;
    *) NAMED+=("$arg") ;;
  esac
done

ALL_TARGETS="frontend server-ui server desktop android docker"
if [ "${#NAMED[@]}" -gt 0 ]; then
  WANTED=("${NAMED[@]}")
  EXPLICIT=1
else
  read -ra WANTED <<< "$ALL_TARGETS"
  EXPLICIT=0
fi

# A missing toolchain is a skip on a default run, a failure when the target
# was named explicitly.
blocked() {
  if [ "$EXPLICIT" -eq 1 ]; then
    FAILED+=("$1: $2")
    err "$1: $2"
  else
    SKIPPED+=("$1: $2")
    skip "$1: $2"
  fi
}

BUILT=()
SKIPPED=()
FAILED=()
INSTALLED=()

version() {
  node -p "require('$REPO_ROOT/package.json').version" 2>/dev/null
}

need() {
  command -v "$1" >/dev/null 2>&1
}

# Cargo needs a host linker named `cc` for build scripts and final binaries.
# Rootless boxes often have no system compiler; the repo family's convention is
# a conda-forge env (see the micromamba recipe in AGENTS.md) — look there too
# and point cargo at whatever we find.
LINKER_OK=0
find_cc() {
  [ "$LINKER_OK" -eq 1 ] && return 0
  local candidate
  if need cc; then LINKER_OK=1; return 0; fi
  use_cc() {
    export "CARGO_TARGET_${HOST_TRIPLE}_LINKER"="$1"
    export CC="$1"
    { [ -x "${1}++" ] || command -v "${1}++" >/dev/null 2>&1; } && export CXX="${1}++"
    # A toolchain dir (conda env etc.) also carries ar/ranlib/ld that C-crate
    # build scripts need — put the whole dir on PATH, not just the compiler.
    case "$1" in
      /*/*) export PATH="$(dirname "$1"):$PATH" ;;
    esac
    LINKER_OK=1
  }
  for candidate in cc gcc clang; do
    if command -v "$candidate" >/dev/null 2>&1; then use_cc "$candidate"; return 0; fi
  done
  for candidate in \
    "$HOME"/opt/mamba/envs/*/bin/clang \
    "$HOME"/opt/mamba/envs/*/bin/*-linux-gnu-cc \
    "${MAMBA_ROOT_PREFIX:-/nonexistent}"/envs/*/bin/clang; do
    if [ -x "$candidate" ]; then use_cc "$candidate"; return 0; fi
  done
  return 1
}

# ---------------------------------------------------------------------------
# frontend: Svelte popup UI. src-tauri embeds build/ via generate_context!,
# so the desktop and android targets call this first when it hasn't run.
# ---------------------------------------------------------------------------
FRONTEND_DONE=0
build_frontend() {
  step "frontend: Svelte UI"
  need npm || { blocked frontend "npm not found"; return 1; }
  if [ ! -d node_modules ] || [ package-lock.json -nt node_modules ]; then
    npm ci || { FAILED+=("frontend: npm ci failed"); err "frontend: npm ci failed"; return 1; }
  fi
  npm run build || { FAILED+=("frontend: vite build failed"); err "frontend: build failed"; return 1; }
  rm -rf "$DIST/frontend"
  mkdir -p "$DIST/frontend"
  cp -R build/. "$DIST/frontend/" \
    || { FAILED+=("frontend: staging to dist/frontend failed"); err "frontend: staging failed"; return 1; }
  BUILT+=("frontend -> dist/frontend")
  FRONTEND_DONE=1
}

build_server_ui() {
  step "server-ui: server admin UI"
  need npm || { blocked server-ui "npm not found"; return 1; }
  if [ ! -d server/ui/node_modules ] || [ server/ui/package-lock.json -nt server/ui/node_modules ]; then
    npm ci --prefix server/ui || { FAILED+=("server-ui: npm ci failed"); err "server-ui: npm ci failed"; return 1; }
  fi
  npm run build --prefix server/ui || { FAILED+=("server-ui: vite build failed"); err "server-ui: build failed"; return 1; }
  rm -rf "$DIST/server-ui"
  mkdir -p "$DIST/server-ui"
  cp -R server/ui/dist/. "$DIST/server-ui/" \
    || { FAILED+=("server-ui: staging to dist/server-ui failed"); err "server-ui: staging failed"; return 1; }
  BUILT+=("server-ui -> dist/server-ui")
}

# ---------------------------------------------------------------------------
# server: pure-Rust axum binary — no GUI system deps needed.
# ---------------------------------------------------------------------------
build_server() {
  step "server: copywraith-server (release)"
  need cargo || { blocked server "cargo not found (install rustup)"; return 1; }
  find_cc || { blocked server "no host C linker (cc/gcc/clang or conda-forge build env)"; return 1; }
  local cargo_args=(-p copywraith-server)
  [ "$DEBUG" -eq 0 ] && cargo_args+=(--release)
  cargo build "${cargo_args[@]}" \
    || { FAILED+=("server: cargo build failed"); err "server: build failed"; return 1; }
  mkdir -p "$DIST"
  local profile=release
  [ "$DEBUG" -eq 1 ] && profile=debug
  cp "target/$profile/copywraith-server" "$DIST/copywraith-server" \
    || { FAILED+=("server: staging to dist failed"); err "server: staging failed"; return 1; }
  BUILT+=("server -> dist/copywraith-server")
}

install_server() {
  step "install: copywraith-server -> ~/.cargo/bin"
  cargo install --path server --force \
    && INSTALLED+=("server -> ~/.cargo/bin/copywraith-server") \
    || FAILED+=("install server: cargo install failed")
}

# ---------------------------------------------------------------------------
# desktop: Tauri bundles for the host OS.
# ---------------------------------------------------------------------------
desktop_toolchain_ok() {
  need cargo || return 1
  need npm || return 1
  find_cc || return 1
  case "$(uname -s)" in
    Linux) pkg-config --exists webkit2gtk-4.1 javascriptcoregtk-4.1 gtk+-3.0 libsoup-3.0 rsvg-2.0 xdo openssl ayatana-appindicator3-0.1 2>/dev/null ;;
    Darwin) need xcodebuild ;;
    *) return 1 ;;
  esac
}

build_desktop() {
  step "desktop: Tauri bundles (host)"
  if ! desktop_toolchain_ok; then
    blocked desktop "missing cargo/npm or webkit2gtk-4.1 dev packages (see README.ubuntu.md)"
    return 1
  fi
  [ "$FRONTEND_DONE" -eq 1 ] || build_frontend || { FAILED+=("desktop: frontend prerequisite failed"); return 1; }
  local args=()
  [ "$DEBUG" -eq 1 ] && args+=(--debug)
  npm run tauri -- build "${args[@]+"${args[@]}"}" \
    || { FAILED+=("desktop: tauri build failed"); err "desktop: build failed"; return 1; }
  if [ "$(uname -s)" = Linux ] && [ "$DEBUG" -eq 0 ]; then
    ./scripts/check-linux-bundle.sh \
      || FAILED+=("desktop: check-linux-bundle.sh failed (see above)")
  fi
  rm -rf "$DIST/desktop"
  mkdir -p "$DIST/desktop"
  local bundle_dir="target/release/bundle"
  [ "$DEBUG" -eq 1 ] && bundle_dir="target/debug/bundle"
  if [ -d "$bundle_dir" ]; then
    cp -R "$bundle_dir"/. "$DIST/desktop/" 2>/dev/null || true
  fi
  if [ -z "$(ls -A "$DIST/desktop" 2>/dev/null)" ]; then
    FAILED+=("desktop: tauri build succeeded but no bundles found under $bundle_dir")
    return 1
  fi
  BUILT+=("desktop -> dist/desktop")
}

install_desktop() {
  step "install: desktop bundle"
  local bundle_dir="$DIST/desktop"
  case "$(uname -s)" in
    Linux)
      local deb
      deb=$(find "$bundle_dir/deb" -name '*.deb' 2>/dev/null | head -n1)
      local appimage
      appimage=$(find "$bundle_dir/appimage" -name '*.AppImage' 2>/dev/null | head -n1)
      if [ -n "$deb" ] && need sudo && need apt-get; then
        sudo apt-get install -y "$deb" \
          && INSTALLED+=("desktop -> apt ($deb)") \
          || FAILED+=("install desktop: apt install failed")
      elif [ -n "$appimage" ]; then
        mkdir -p "$HOME/Applications"
        cp "$appimage" "$HOME/Applications/" && chmod +x "$HOME/Applications/$(basename "$appimage")" \
          && INSTALLED+=("desktop -> ~/Applications/$(basename "$appimage")") \
          || FAILED+=("install desktop: AppImage copy failed")
      elif [ -n "$deb" ]; then
        FAILED+=("install desktop: deb built but sudo is unavailable; install it manually")
      else
        FAILED+=("install desktop: no .deb or .AppImage found under dist/desktop")
      fi
      ;;
    Darwin)
      local app
      app=$(find "$bundle_dir/macos" -name '*.app' 2>/dev/null | head -n1)
      if [ -n "$app" ]; then
        rm -rf "/Applications/$(basename "$app")" 2>/dev/null || true
        cp -R "$app" /Applications/ \
          && INSTALLED+=("desktop -> /Applications/$(basename "$app")") \
          || FAILED+=("install desktop: copy to /Applications failed")
      else
        FAILED+=("install desktop: no .app found under dist/desktop")
      fi
      ;;
    *)
      FAILED+=("install desktop: unsupported host OS: $(uname -s)")
      ;;
  esac
}

# ---------------------------------------------------------------------------
# android: universal release APK for the four ABIs.
# ---------------------------------------------------------------------------
# A `java` on PATH can be a broken shim (macOS's /usr/bin/java stub exits
# non-zero), and Gradle needs JDK 17+. Verify the binary runs and is new
# enough before accepting it.
java_ok() {
  local major
  # JEP 223 GA builds print a dotless major ("version "17""), so the
  # captured major must end on any non-digit, not a literal dot.
  major=$("$1" -version 2>&1 | sed -n 's/.*version "\([0-9][0-9]*\)[^0-9].*/\1/p' | head -n1)
  case "$major" in ''|*[!0-9]*) return 1 ;; esac
  [ "$major" -ge 17 ]
}

find_java() {
  if need java && java_ok java; then return 0; fi
  local candidate
  for candidate in \
    "${JAVA_HOME:-}/bin/java" \
    "$HOME"/opt/jdk-*/bin/java \
    "$HOME"/opt/jdk*/bin/java \
    /usr/lib/jvm/*/bin/java \
    "$HOME"/Applications/Android\ Studio.app/Contents/jbr/*/Contents/Home/bin/java \
    /opt/android-studio/jbr/bin/java; do
    if [ -x "$candidate" ] && java_ok "$candidate"; then
      export JAVA_HOME="${candidate%/bin/java}"
      export PATH="$JAVA_HOME/bin:$PATH"
      return 0
    fi
  done
  return 1
}

find_android_sdk() {
  local candidate
  for candidate in \
    "${ANDROID_HOME:-}" \
    "${ANDROID_SDK_ROOT:-}" \
    "$HOME/Android/Sdk" \
    "$HOME/Library/Android/sdk"; do
    if [ -n "$candidate" ] && [ -d "$candidate/platform-tools" ]; then
      export ANDROID_HOME="$candidate"
      return 0
    fi
  done
  return 1
}

find_android_ndk() {
  local ndk
  ndk=$(ls -d "$ANDROID_HOME"/ndk/*/ 2>/dev/null | sort -t. -k1,1n -k2,2n | tail -n1)
  [ -n "$ndk" ] || return 1
  export ANDROID_NDK_HOME="${ndk%/}"
}

# Map the CW_ANDROID_TARGETS ABI names to the rustup triples they need.
abi_triple() {
  case "$1" in
    aarch64) echo "aarch64-linux-android" ;;
    armv7)   echo "armv7-linux-androideabi" ;;
    i686)    echo "i686-linux-android" ;;
    x86_64)  echo "x86_64-linux-android" ;;
    *)       echo "$1" ;;
  esac
}

build_android() {
  step "android: universal APK"
  find_java || { blocked android "no JDK 17 (JAVA_HOME unset; nothing in ~/opt, /usr/lib/jvm, Android Studio jbr)"; return 1; }
  find_android_sdk || { blocked android "no Android SDK (ANDROID_HOME unset; nothing under ~/Android/Sdk)"; return 1; }
  find_android_ndk || { blocked android "no NDK under $ANDROID_HOME/ndk"; return 1; }
  need cargo || { blocked android "cargo not found"; return 1; }
  need npm || { blocked android "npm not found"; return 1; }
  find_cc || { blocked android "no host C linker (needed by cargo build scripts)"; return 1; }
  local targets="${CW_ANDROID_TARGETS:-aarch64 armv7 i686 x86_64}"
  if command -v rustup >/dev/null 2>&1; then
    local abi
    for abi in $targets; do
      rustup target list --installed 2>/dev/null | grep -qx "$(abi_triple "$abi")" \
        || { blocked android "rust target $(abi_triple "$abi") not installed (run scripts/android-dev-bootstrap.sh)"; return 1; }
    done
  fi
  note "java: $(java -version 2>&1 | head -n1)"
  note "sdk:  $ANDROID_HOME | ndk: $ANDROID_NDK_HOME"
  [ "$FRONTEND_DONE" -eq 1 ] || build_frontend || { FAILED+=("android: frontend prerequisite failed"); return 1; }

  if [ ! -d src-tauri/gen/android ]; then
    step "android: tauri android init (src-tauri/gen missing)"
    npm run tauri -- android init || { FAILED+=("android: tauri android init failed"); return 1; }
  fi

  local args=(android build --apk)
  [ "$DEBUG" -eq 1 ] && args+=(--debug)
  # shellcheck disable=SC2086
  npm run tauri -- "${args[@]}" --target $targets \
    || { FAILED+=("android: tauri android build failed"); err "android: build failed"; return 1; }

  rm -rf "$DIST/android"
  mkdir -p "$DIST/android"
  local profile=release
  [ "$DEBUG" -eq 1 ] && profile=debug
  # find -exec \; never propagates cp's exit status — a read-loop does,
  # matching the hard-fail staging of the other targets.
  local apk
  while IFS= read -r apk; do
    cp "$apk" "$DIST/android/" \
      || { FAILED+=("android: failed to copy $(basename "$apk") into dist/android"); return 1; }
  done < <(find "src-tauri/gen/android/app/build/outputs/apk" \
            -name '*.apk' -path "*$profile*")
  if ! ls "$DIST/android"/*.apk >/dev/null 2>&1; then
    FAILED+=("android: no APK found under outputs/apk/$profile")
    return 1
  fi
  BUILT+=("android -> dist/android ($(ls "$DIST/android" | tr '\n' ' '))")
}

install_android() {
  step "install: APK -> connected device"
  local adb="${ANDROID_HOME:-}/platform-tools/adb"
  [ -x "$adb" ] || adb=$(command -v adb 2>/dev/null || true)
  [ -n "$adb" ] || { FAILED+=("install android: adb not found"); return 1; }
  if ! "$adb" devices | grep -q "device$"; then
    FAILED+=("install android: no device/emulator connected")
    return 1
  fi
  local apk
  apk=$(ls -t "$DIST"/android/*.apk 2>/dev/null | head -n1)
  [ -n "$apk" ] || { FAILED+=("install android: no APK in dist/android"); return 1; }
  "$adb" install -r "$apk" \
    && INSTALLED+=("android -> $(basename "$apk") via adb") \
    || FAILED+=("install android: adb install failed")
}

# ---------------------------------------------------------------------------
# docker: server image.
# ---------------------------------------------------------------------------
build_docker() {
  step "docker: copywraith-server image"
  need docker || { blocked docker "docker not found"; return 1; }
  local tag="copywraith-server:$(version)"
  docker build -f server/Dockerfile -t "$tag" "$REPO_ROOT" \
    || { FAILED+=("docker: docker build failed"); err "docker: build failed"; return 1; }
  BUILT+=("docker -> image $tag")
}

install_docker() {
  step "install: local stack via docker compose"
  docker compose up -d --build \
    && INSTALLED+=("docker -> compose up -d") \
    || FAILED+=("install docker: docker compose up failed")
}

# Exact-match check that a target reached BUILT (the entries look like
# "server -> dist/copywraith-server"; substring matching would confuse
# "server" with "server-ui").
built_target() {
  local b
  for b in "${BUILT[@]+"${BUILT[@]}"}"; do
    [ "${b%% ->*}" = "$1" ] && return 0
  done
  return 1
}

# ---------------------------------------------------------------------------
for target in "${WANTED[@]}"; do
  case "$target" in
    frontend)  build_frontend ;;
    server-ui) build_server_ui ;;
    server)    build_server ;;
    desktop)   build_desktop ;;
    android)   build_android ;;
    docker)    build_docker ;;
    *) err "unknown target: $target"; FAILED+=("$target: unknown target") ;;
  esac
done

if [ "$INSTALL" -eq 1 ]; then
  for target in "${WANTED[@]}"; do
    case "$target" in
      server)   built_target server && install_server ;;
      desktop)  built_target desktop && install_desktop ;;
      android)  built_target android && install_android ;;
      docker)   built_target docker && install_docker ;;
      frontend|server-ui)
        built_target "$target" && note "install: $target has nothing to install" ;;
    esac
  done
fi

# ---------------------------------------------------------------------------
step "Summary"
for line in "${BUILT[@]+"${BUILT[@]}"}"; do note "built:    $line"; done
for line in "${INSTALLED[@]+"${INSTALLED[@]}"}"; do note "installed: $line"; done
for line in "${SKIPPED[@]+"${SKIPPED[@]}"}"; do note "skipped:  $line"; done
for line in "${FAILED[@]+"${FAILED[@]}"}"; do err "failed:   $line"; done

[ "${#FAILED[@]}" -eq 0 ] || exit 1
[ "${#BUILT[@]}" -eq 0 ] && { err "nothing built"; exit 1; }
if [ "$(uname -s)" = Darwin ]; then open "$DIST" 2>/dev/null || true; fi
exit 0
