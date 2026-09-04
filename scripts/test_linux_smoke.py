"""Keep smoke-test failures attributable to the client, not its diagnostics."""

import importlib.util
from pathlib import Path
import subprocess
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

    def test_diagnostic_errors_are_best_effort(self):
        errors = [FileNotFoundError("missing tool"), subprocess.TimeoutExpired("scrot", 5)]
        for error in errors:
            with self.subTest(error=error), patch.object(smoke.subprocess, "run", side_effect=error):
                smoke.diagnose("scrot")


if __name__ == "__main__":
    unittest.main()
