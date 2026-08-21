#!/usr/bin/env bash
# Verify the Linux bundles Tauri just produced.
#
# The interesting part is the binary name: the Ubuntu/KDE docs tell users to
# bind `copywraith --toggle` to a global shortcut, and Copywraith itself writes
# `Icon=copywraith` into its autostart entry. Both silently break if the bundled
# binary is ever renamed (it used to ship as `copywraith-tauri`), so assert it
# here instead of finding out from a bug report.
set -euo pipefail

BUNDLE_DIR="${1:-target/release/bundle}"
EXPECTED_BIN="copywraith"

fail() {
	echo "::error::$1"
	exit 1
}

deb=$(find "$BUNDLE_DIR/deb" -name '*.deb' | head -n1)
[ -n "$deb" ] || fail "No .deb was produced in $BUNDLE_DIR/deb"
echo "Checking $deb"

contents=$(dpkg-deb -c "$deb")
grep -qE " \.?/?usr/bin/$EXPECTED_BIN$" <<<"$contents" ||
	fail "The .deb does not install /usr/bin/$EXPECTED_BIN — the documented \`$EXPECTED_BIN --toggle\` shortcut command would not exist. Check \`mainBinaryName\` in src-tauri/tauri.conf.json."

grep -qE "usr/share/icons/.*/$EXPECTED_BIN\.png$" <<<"$contents" ||
	fail "The .deb does not install an icon named $EXPECTED_BIN.png — autostart entries and paste notifications reference \`Icon=$EXPECTED_BIN\`."

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT
dpkg-deb -x "$deb" "$workdir"

desktop=$(find "$workdir/usr/share/applications" -name '*.desktop' | head -n1)
[ -n "$desktop" ] || fail "The .deb does not ship a .desktop entry"
grep -qx "Exec=$EXPECTED_BIN" "$desktop" ||
	fail "The .desktop entry does not exec \`$EXPECTED_BIN\`: $(grep '^Exec=' "$desktop")"

appimage=$(find "$BUNDLE_DIR/appimage" -name '*.AppImage' 2>/dev/null | head -n1 || true)
if [ -n "$appimage" ]; then
	echo "Checking $appimage"
	[ -x "$appimage" ] || fail "The AppImage is not executable"
fi

echo "Linux bundle checks passed."
