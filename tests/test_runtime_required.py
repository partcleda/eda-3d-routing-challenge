"""Runtime is required: CI's metadata check and the leaderboard's handling."""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

import m3d.cli as cli

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT = os.path.join(REPO, "scripts", "check_submission_meta.py")
SEED = os.path.join("submissions", "hard", "negotiated")


def _run(*dirs):
    return subprocess.run([sys.executable, SCRIPT, *dirs], capture_output=True, text=True)


class TestCheckSubmissionMeta(unittest.TestCase):
    def _entry(self, tmp, runtime=True, meta=None):
        d = os.path.join(tmp, "submissions", "hard", "x")
        shutil.copytree(SEED, d)
        if not runtime:
            os.remove(os.path.join(d, "runtime.json"))
        with open(os.path.join(d, "meta.json"), "w", encoding="utf-8") as fh:
            json.dump(meta if meta is not None else
                      {"author": "a", "hardware": "laptop", "threads": 4}, fh)
        return d

    def test_complete_entry_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(_run(self._entry(tmp)).returncode, 0)

    def test_missing_runtime_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = _run(self._entry(tmp, runtime=False))
            self.assertEqual(p.returncode, 1)
            self.assertIn("runtime.json is missing", p.stdout)

    def test_partial_runtime_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            d = self._entry(tmp)
            with open(os.path.join(d, "runtime.json"), "w", encoding="utf-8") as fh:
                json.dump({"case_01": 1.0, "case_02": -1}, fh)
            p = _run(d)
            self.assertEqual(p.returncode, 1)
            self.assertIn("case_02", p.stdout)
            self.assertIn("case_09", p.stdout)

    def test_hardware_and_threads_required(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = _run(self._entry(tmp, meta={"author": "a", "threads": "8"}))
            self.assertEqual(p.returncode, 1)
            self.assertIn('"hardware"', p.stdout)
            self.assertIn('"threads"', p.stdout)


class TestRuntimeOnLeaderboard(unittest.TestCase):
    def _root(self, tmp):
        for name in ("timed", "untimed"):
            shutil.copytree(SEED, os.path.join(tmp, "hard", name))
        os.remove(os.path.join(tmp, "hard", "untimed", "runtime.json"))
        return tmp

    def _row(self, md, name):
        return [l for l in md.splitlines() if l.startswith("| ") and f"| {name} |" in l][0]

    def test_untimed_entries_are_unranked(self):
        with tempfile.TemporaryDirectory() as tmp:
            md = cli._render_leaderboard_md(self._root(tmp))
            self.assertIn("| 1.0000 |", self._row(md, "timed"))
            row = self._row(md, "untimed")
            self.assertTrue(row.startswith("| 2 |"))
            self.assertIn("| missing |", row)
            self.assertIn("| — |", row)

if __name__ == "__main__":
    unittest.main()
