"""CI's copy check: route files identical to another entry's are rejected."""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "scripts"))
import check_copies  # noqa: E402

SEED = os.path.join(REPO, "submissions", "hard", "negotiated")


class TestCheckCopies(unittest.TestCase):
    def _root(self, tmp):
        for name in ("upstream", "mine"):
            shutil.copytree(SEED, os.path.join(tmp, "hard", name))
        return tmp

    def test_identical_routes_are_copies(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp)
            found = check_copies.problems(os.path.join(root, "hard", "mine"))
            self.assertEqual(len(found), 9)
            self.assertIn("routes identical to upstream", found[0])

    def test_reordered_routes_are_still_copies(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp)
            path = os.path.join(root, "hard", "mine", "case_01.sol.json")
            with open(path, encoding="utf-8") as fh:
                doc = json.load(fh)
            doc["routes"] = [dict(r, edges=[e[::-1] for e in r["edges"][::-1]])
                             for r in doc["routes"][::-1]]
            with open(path, "w", encoding="utf-8") as fh:
                json.dump(doc, fh)
            found = check_copies.problems(os.path.dirname(path))
            self.assertTrue(any(p.startswith("case_01:") for p in found))

    def test_own_routes_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp)
            shutil.rmtree(os.path.join(root, "hard", "upstream"))
            self.assertEqual(check_copies.problems(os.path.join(root, "hard", "mine")), [])

    def test_file_argument_checks_only_that_case(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp)
            found = check_copies.problems(os.path.join(root, "hard", "mine", "case_02.sol.json"))
            self.assertEqual(len(found), 1)
            self.assertTrue(found[0].startswith("case_02:"))

    def test_cli_exit_code(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = self._root(tmp)
            mine = os.path.join(root, "hard", "mine")
            p = subprocess.run([sys.executable, os.path.join(REPO, "scripts", "check_copies.py"), mine],
                               capture_output=True, text=True)
            self.assertEqual(p.returncode, 1)
            self.assertIn("routes identical to upstream", p.stdout)

if __name__ == "__main__":
    unittest.main()
