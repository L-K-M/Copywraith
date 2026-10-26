# Third-party license texts

Inputs for the notice generators (see `AGENTS.md`, "Licensing"). Each file's
origin is recorded next to where it is used.

| Path | Used by | Origin |
| --- | --- | --- |
| `rust-overrides.json`, `spdx/`, `webview2/` | `scripts/rust-notices.py` | `spdx/`: SPDX License List v3.29.0. `webview2/`: `LICENSE.txt` and `NOTICE.txt` from the Microsoft.Web.WebView2 1.0.3800.47 NuGet package. Copyright lines: see each override's `reason`. |
| `fonts/` | `scripts/third-party-notices.mjs` | OFL 1.1 template from openfontlicense.org with each font's copyright line from its name table. |
| `js/` | `scripts/third-party-notices.mjs` | `LICENSE` files from the npm tarballs named in `third-party-notices.jsonc`. |
| `android/` | AboutLibraries (`scripts/add-android-notices.sh`), `scripts/android_notices.py` | License and NOTICE texts; each JSON file's `_comment` names its source. |
| `appimage/` | `scripts/appimage-notices.py` | Upstream texts listed in `appimage-notices.json`. |
| `linux/` | `scripts/deb-copyright.py` | Spec for the DEP-5 `copyright` file; it reads `LICENSE` and the building toolchain's Rust license texts. |
