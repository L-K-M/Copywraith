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
# Third-party notices (Tauri bundle files in src-tauri/tauri.conf.json): the
# deb package name is the doc directory.
DOC_DIR="usr/share/doc/copywraith"

fail() {
	echo "::error::$1"
	exit 1
}

# The aggregated notices name a crate and the OFL fonts that are certain to ship;
# Rust-Licenses.html is the toolchain's COPYRIGHT-library.html.
check_notices() {
	local root="$1" what="$2"
	local notices="$root/$DOC_DIR/Third-Party-Notices.txt" std="$root/$DOC_DIR/Rust-Licenses.html"
	[ -s "$notices" ] || fail "$what does not ship /$DOC_DIR/Third-Party-Notices.txt"
	[ -s "$std" ] || fail "$what does not ship /$DOC_DIR/Rust-Licenses.html"
	grep -q '^tauri [0-9]' "$notices" || fail "$what: Third-Party-Notices.txt lists no tauri crate"
	grep -q 'SIL OPEN FONT LICENSE Version 1.1' "$notices" || fail "$what: Third-Party-Notices.txt lacks the fonts' OFL text"
	grep -q 'Copyright notices for The Rust Standard Library' "$std" || fail "$what: Rust-Licenses.html is not the Rust standard library notices"
}

# The DEP-5 copyright file from scripts/deb-copyright.py (deb and AppImage; the
# rpm states its license in the package header instead).
check_copyright() {
	local root="$1" what="$2"
	local copyright="$root/$DOC_DIR/copyright"
	[ -s "$copyright" ] || fail "$what does not ship /$DOC_DIR/copyright"
	grep -qx 'Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/' "$copyright" ||
		fail "$what: /$DOC_DIR/copyright is not a DEP-5 file"
	grep -qx 'License: Unlicense' "$copyright" || fail "$what: /$DOC_DIR/copyright does not state the Unlicense"
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
check_notices "$workdir" "The .deb"
check_copyright "$workdir" "The .deb"

rpm=$(find "$BUNDLE_DIR/rpm" -name '*.rpm' 2>/dev/null | head -n1 || true)
if [ -n "$rpm" ]; then
	echo "Checking $rpm"
	command -v bsdtar >/dev/null || fail "bsdtar (libarchive-tools) is needed to unpack the .rpm"
	mkdir "$workdir/rpm"
	bsdtar -xf "$rpm" -C "$workdir/rpm" || fail "The .rpm could not be unpacked"
	check_notices "$workdir/rpm" "The .rpm"
fi

appimage=$(find "$BUNDLE_DIR/appimage" -name '*.AppImage' 2>/dev/null | head -n1 || true)
if [ -n "$appimage" ]; then
	echo "Checking $appimage"
	[ -x "$appimage" ] || fail "The AppImage is not executable"
	# scripts/appimage-notices.py adds the bundled system libraries' copyright
	# files, their source packages (SOURCES.txt) and the AppImage runtime.
	appimage_path=$(readlink -f "$appimage")
	(cd "$workdir" && "$appimage_path" --appimage-extract >/dev/null) ||
		fail "The AppImage could not be extracted"
	check_notices "$workdir/squashfs-root" "The AppImage"
	check_copyright "$workdir/squashfs-root" "The AppImage"
	grep -q 'Bundled system libraries' "$workdir/squashfs-root/$DOC_DIR/Third-Party-Notices.txt" ||
		fail "The AppImage's notices lack the bundled system libraries; run scripts/appimage-notices.py"
	grep -q 'type2-runtime' "$workdir/squashfs-root/$DOC_DIR/Third-Party-Notices.txt" ||
		fail "The AppImage's notices do not describe the AppImage runtime"
	[ -s "$workdir/squashfs-root/$DOC_DIR/SOURCES.txt" ] || fail "The AppImage does not ship /$DOC_DIR/SOURCES.txt"
	[ -s "$workdir/squashfs-root/usr/share/doc/libwebkit2gtk-4.1-0/copyright" ] ||
		fail "The AppImage does not ship WebKitGTK's copyright file"
fi

echo "Linux bundle checks passed."
