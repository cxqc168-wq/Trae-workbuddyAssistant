"""Integration regression for a browser exiting during Windows cleanup."""
import importlib.util
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('release_runtime', ROOT / 'scripts/verify-release-runtime.py')
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)


class BrowserCleanupRaceTest(unittest.TestCase):
    def test_browser_already_exited_at_taskkill_does_not_fail_runtime_verification(self):
        original_run = subprocess.run
        race_exercised = []

        def run_with_exit_race(command, *args, **kwargs):
            if Path(command[0]).name.lower() == 'taskkill.exe':
                # Kill only the exact tree the verifier owns, just before its
                # kill command runs: the OS now reports that PID is gone.
                first = original_run(command, capture_output=True, timeout=15, check=False)
                self.assertEqual(first.returncode, 0, 'Could not induce the cleanup race')
                race_exercised.append(True)
            return original_run(command, *args, **kwargs)

        failure = None
        with patch.object(runtime.subprocess, 'run', side_effect=run_with_exit_race):
            try:
                runtime.verify(ROOT / 'resources')
            except subprocess.CalledProcessError as error:
                failure = error
        self.assertTrue(race_exercised, 'Browser cleanup race was not exercised')
        self.assertIsNone(failure, f'An already exited browser caused a false failure: {failure}')


if __name__ == '__main__':
    unittest.main()
