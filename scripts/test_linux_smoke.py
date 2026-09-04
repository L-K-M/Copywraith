"""Keep smoke-test failures attributable to the client, not its diagnostics."""

import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location(
    "linux_smoke", Path(__file__).with_name("smoke-linux-client.py")
)
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


class SmokeTest(unittest.TestCase):
    def test_exit_during_condition_cannot_pass(self):
        client = Mock()
        client.poll.side_effect = [None, 1]
        client.returncode = 1
        with patch.object(smoke.time, "sleep"):
            with self.assertRaisesRegex(AssertionError, "Client exited"):
                smoke.wait_for("popup disappeared", lambda: True, client)

    def test_detaches_only_the_private_document_mount(self):
        with tempfile.TemporaryDirectory() as directory:
            portal = Path(directory) / "doc"
            portal.mkdir()
            with patch.object(smoke.shutil, "which", return_value="fusermount3"):
                with patch.object(smoke, "diagnose") as diagnose:
                    smoke.unmount_document_portal(directory)
                    diagnose.assert_called_once_with("fusermount3", "-uz", str(portal))

    def test_no_unmount_without_a_document_portal(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(smoke, "diagnose") as diagnose:
                smoke.unmount_document_portal(directory)
                diagnose.assert_not_called()

    def test_diagnostic_errors_are_best_effort(self):
        errors = [FileNotFoundError("missing tool"), subprocess.TimeoutExpired("scrot", 5)]
        for error in errors:
            with self.subTest(error=error), patch.object(smoke.subprocess, "run", side_effect=error):
                smoke.diagnose("scrot")


if __name__ == "__main__":
    unittest.main()
