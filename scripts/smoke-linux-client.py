#!/usr/bin/env python3
"""Exercise the installed client in a disposable X11 desktop, not a user's session."""

import argparse
from contextlib import contextmanager
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time

WAIT_SECONDS = 30
POLL_SECONDS = 0.2
COMMAND_SECONDS = 5
UI_SETTLE_SECONDS = 1
PASTE_SETTLE_SECONDS = 1
SESSION_SECONDS = 300
APP_ID = "ch.lkmc.copywraith"
FIRST_TEXT = "Copywraith Ubuntu smoke first entry"
SECOND_TEXT = "Copywraith Ubuntu smoke second entry"


def run(*args, **kwargs):
    return subprocess.run(
        args, check=True, text=True, timeout=COMMAND_SECONDS, **kwargs
    )


@contextmanager
def process(*args, log):
    child = subprocess.Popen(args, stdout=log, stderr=log)
    try:
        yield child
    finally:
        if child.poll() is None:
            child.terminate()
        try:
            child.wait(timeout=COMMAND_SECONDS)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()


def wait_for(description, condition, client):
    deadline = time.monotonic() + WAIT_SECONDS
    while time.monotonic() < deadline:
        assert client.poll() is None, f"Client exited: {client.returncode}"
        if condition():
            print(f"PASS: {description}", flush=True)
            # Respect the client's toggle debounce between successive actions.
            time.sleep(POLL_SECONDS)
            return
        time.sleep(POLL_SECONDS)
    raise AssertionError(f"Timed out: {description}")


def popup_visible(client):
    result = subprocess.run(
        ["xdotool", "search", "--onlyvisible", "--pid", str(client.pid),
         "--name", "^Copywraith$"],
        text=True, capture_output=True, timeout=COMMAND_SECONDS,
    )
    assert result.returncode in (0, 1), result.stderr
    return bool(result.stdout.strip())


def history_texts():
    database = Path(os.environ["XDG_DATA_HOME"]) / APP_ID / "copywraith.db"
    if not database.exists():
        return []
    with sqlite3.connect(f"{database.as_uri()}?mode=ro", uri=True) as connection:
        return [row[0] for row in connection.execute(
            "SELECT text_plain FROM entries ORDER BY updated_at DESC"
        )]


def copy_text(text):
    run("xclip", "-selection", "clipboard", input=text,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def clipboard_text():
    return run("xclip", "-selection", "clipboard", "-o", capture_output=True).stdout


def press(key):
    run("xdotool", "key", "--clearmodifiers", key)


def exercise(binary, client, log_path):
    wait_for("clipboard monitor starts",
             lambda: "Clipboard monitor started" in log_path.read_text(), client)
    assert not popup_visible(client), "Tray app should start hidden"

    # Capture real clipboard changes; no seeded database or mock clipboard.
    for text in (FIRST_TEXT, SECOND_TEXT):
        copy_text(text)
        wait_for(f"capture {text}", lambda: text in history_texts(), client)

    # A second launch must dispatch to the existing process and exit promptly.
    run(binary, "--toggle")
    wait_for("forwarded toggle opens popup", lambda: popup_visible(client), client)

    # Escape is handled in Svelte, proving the packaged webview loaded too.
    def escape_hides_popup():
        press("Escape")
        return not popup_visible(client)

    wait_for("frontend Escape hides popup", escape_hides_popup, client)
    press("ctrl+shift+v")
    wait_for("X11 global shortcut opens popup", lambda: popup_visible(client), client)
    wait_for("frontend closes popup again", escape_hides_popup, client)

    run(binary, "--starred")
    wait_for("forwarded starred command opens popup",
             lambda: popup_visible(client), client)
    wait_for("close starred popup", escape_hides_popup, client)

    # Search and paste an older entry through the UI, not a backend test hook.
    run(binary, "--toggle")
    wait_for("open history for search", lambda: popup_visible(client), client)
    time.sleep(UI_SETTLE_SECONDS)  # The popup-show handler focuses search.
    run("xdotool", "type", "--clearmodifiers", FIRST_TEXT)

    def selection_pasted():
        press("Return")
        return not popup_visible(client)

    wait_for("search result pastes and closes popup", selection_pasted, client)
    assert clipboard_text() == FIRST_TEXT, "Search pasted the wrong entry"

    # Empty clipboard content is ignored by history; CLI paste restores history.
    time.sleep(PASTE_SETTLE_SECONDS)  # Let self-write suppression expire.
    expected = history_texts()[0]
    copy_text("")
    run(binary, "--paste-plaintext")
    wait_for("forwarded plaintext paste restores clipboard",
             lambda: clipboard_text() == expected, client)
    assert len(history_texts()) == 2, "Pasting duplicated clipboard history"


def session(binary):
    log_path = Path(os.environ["HOME"]) / "client.log"
    with log_path.open("w") as log:
        with process("openbox", log=log), process(binary, log=log) as client:
            try:
                exercise(binary, client, log_path)
            finally:
                print(log_path.read_text(), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", nargs="?", default="/usr/bin/copywraith")
    parser.add_argument("--session", action="store_true", help=argparse.SUPPRESS)
    args = parser.parse_args()
    binary = str(Path(args.binary).resolve())
    if args.session:
        session(binary)
        return

    # Isolate history, shortcuts, the instance socket, D-Bus, and display.
    with tempfile.TemporaryDirectory(prefix="copywraith-smoke-") as directory:
        env = os.environ.copy()
        for key in ("APPIMAGE", "APPDIR", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS"):
            env.pop(key, None)
        for key, subdir in {
            "HOME": "home", "XDG_DATA_HOME": "data", "XDG_CONFIG_HOME": "config",
            "XDG_CACHE_HOME": "cache", "XDG_RUNTIME_DIR": "runtime",
        }.items():
            path = Path(directory) / subdir
            path.mkdir(mode=0o700)
            env[key] = str(path)
        env.update(GDK_BACKEND="x11", XDG_SESSION_TYPE="x11",
                   XDG_CURRENT_DESKTOP="Openbox", RUST_LOG="info",
                   LIBGL_ALWAYS_SOFTWARE="1")
        subprocess.run(
            ["dbus-run-session", "--", "xvfb-run", "--auto-servernum",
             sys.executable, str(Path(__file__).resolve()), binary, "--session"],
            env=env, check=True, timeout=SESSION_SECONDS,
        )


if __name__ == "__main__":
    main()
