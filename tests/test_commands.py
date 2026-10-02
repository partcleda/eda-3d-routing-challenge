"""The documented commands: CLI subcommands, the example router, and resumable
tier builds behave as the README / CONTRIBUTING describe."""
import contextlib
import importlib.util
import io
import json
import os
import shutil
import sys
import tempfile
import unittest
from unittest import mock

from m3d import baseline, designs, negotiated
from m3d.checker import check
from m3d.cli import main
from m3d.generator import GenConfig
from m3d.model import Instance
from m3d.suite import MASTER_SEED

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
INTRO = os.path.join(REPO, "benchmarks")

TINY_BLIF = """
.model t
.inputs a b c
.outputs f
.names a b g
11 1
.names g c f
11 1
.end
"""


def _load_example():
    path = os.path.join(REPO, "examples", "example_submission.py")
    spec = importlib.util.spec_from_file_location("example_submission", path)
    mod = importlib.util.module_from_spec(spec)
    sys.modules["example_submission"] = mod
    spec.loader.exec_module(mod)
    return mod


def _run(argv):
    """Run the CLI in-process; return (exit code, stdout, stderr)."""
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        rc = main(argv)
    return rc, out.getvalue(), err.getvalue()


def _mini_suite(tmp, routers):
    """A suite dir holding intro cases case_01.. with the given baseline_router
    per case (None = no key, like an older manifest)."""
    with open(os.path.join(INTRO, "suite.json")) as fh:
        man = json.load(fh)
    cases = []
    for c, router in zip(man["cases"], routers):
        shutil.copy(os.path.join(INTRO, c["instance_file"]), tmp)
        c = {k: v for k, v in c.items() if k != "baseline_router"}
        if router is not None:
            c["baseline_router"] = router
        cases.append(c)
    with open(os.path.join(tmp, "suite.json"), "w") as fh:
        json.dump({**man, "n_cases": len(cases), "cases": cases}, fh)
    return tmp


class TestBaselineSuite(unittest.TestCase):
    def test_uses_the_router_named_in_the_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            suite = _mini_suite(tmp, ["negotiated", "baseline", None])
            out_dir = os.path.join(tmp, "out")
            with mock.patch("m3d.negotiated.route_negotiated",
                            wraps=negotiated.route_negotiated) as neg, \
                    mock.patch("m3d.cli.route", wraps=baseline.route) as simple:
                rc, out, _ = _run(["baseline-suite", "--suite", suite,
                                   "--out-dir", out_dir])
            self.assertEqual(rc, 0, out)
            # same call (and net order) the generator used to certify the reference
            calls = [(c.args[0].name, c.kwargs.get("order")) for c in neg.call_args_list]
            self.assertEqual(calls, [("case_01", GenConfig().baseline_order)])
            self.assertEqual([c.args[0].name for c in simple.call_args_list],
                             ["case_02", "case_03"])
            self.assertEqual(sorted(os.listdir(out_dir)),
                             ["case_01.sol.json", "case_02.sol.json", "case_03.sol.json"])


class TestRunSuite(unittest.TestCase):
    def test_reports_only_the_solutions_it_wrote(self):
        real_route = baseline.route

        def fail_case_02(inst, **kw):
            return (None, None) if inst.name == "case_02" else real_route(inst, **kw)

        with tempfile.TemporaryDirectory() as tmp:
            suite = _mini_suite(tmp, [None, None])
            out_dir = os.path.join(tmp, "out")
            with mock.patch("m3d.cli.route", side_effect=fail_case_02):
                rc, out, _ = _run(["run-suite", "--suite", suite, "--out-dir", out_dir])
            self.assertEqual(rc, 3)
            self.assertEqual(sorted(os.listdir(out_dir)),
                             ["case_01.sol.json", "runtime.json"])
            self.assertIn("wrote 1/2 solutions", out)


class TestExampleSubmission(unittest.TestCase):
    def test_falls_back_to_negotiated_when_baseline_fails(self):
        ex = _load_example()
        inst = Instance.load(os.path.join(INTRO, "case_01.json"))
        with mock.patch.object(ex, "greedy_route", return_value=None), \
                mock.patch("m3d.baseline.route", return_value=(None, None)) as simple, \
                mock.patch("m3d.negotiated.route_negotiated",
                           wraps=negotiated.route_negotiated) as neg:
            sub = ex.route_instance(inst)
        self.assertTrue(simple.called)
        self.assertTrue(neg.called)
        self.assertTrue(check(inst, sub).legal)


class TestDesignsResume(unittest.TestCase):
    def test_rebuilds_when_layers_or_master_seed_change(self):
        specs = [{"name": "tiny", "blif": "tiny.blif", "channel": 3}]
        with tempfile.TemporaryDirectory() as tmp:
            with open(os.path.join(tmp, "tiny.blif"), "w") as fh:
                fh.write(TINY_BLIF)
            out = os.path.join(tmp, "suite")
            inst_path = os.path.join(out, "tiny.json")

            def build(layers, master_seed):
                return designs.build_design_suite(out, blif_dir=tmp, layers=layers,
                                                  master_seed=master_seed, verbose=False)

            with mock.patch.object(designs, "DESIGN_SPECS", specs), \
                    mock.patch("m3d.designs.generate_design_feasible",
                               wraps=designs.generate_design_feasible) as gen:
                build(6, 7)
                self.assertEqual(gen.call_count, 1)
                build(6, 7)                       # same config: resumed, not rebuilt
                self.assertEqual(gen.call_count, 1)
                man = build(4, 7)                 # different layer count: rebuilt
                self.assertEqual(gen.call_count, 2)
                self.assertEqual(Instance.load(inst_path).layers, 4)
                self.assertEqual(man["cases"][0]["layers"], 4)
                build(4, 8)                       # different master seed: rebuilt
                self.assertEqual(gen.call_count, 3)
                self.assertEqual(Instance.load(inst_path).master_seed, 8)

    def test_committed_tier_is_reused_with_default_settings(self):
        # `generate --tier designs` (6 layers, default master seed) must keep
        # resuming from the committed instances instead of re-certifying them
        with tempfile.TemporaryDirectory() as tmp:
            out = os.path.join(tmp, "designs")
            shutil.copytree(os.path.join(REPO, designs.DESIGN_DIR), out)
            with mock.patch("m3d.designs.generate_design_feasible",
                            side_effect=AssertionError("should have reused")):
                man = designs.build_design_suite(out, layers=6, master_seed=MASTER_SEED,
                                                 verbose=False)
            self.assertEqual(len(man["cases"]), len(designs.DESIGN_SPECS))


class TestCliArgumentErrors(unittest.TestCase):
    def test_animate_layers_requires_case(self):
        rc, _, err = _run(["animate", "--mode", "layers"])
        self.assertEqual(rc, 2)
        self.assertIn("--case", err)

    def test_visualize_rejects_out_of_range_layer(self):
        case = os.path.join(INTRO, "case_01.json")     # 6 layers: z = 0..5
        with tempfile.TemporaryDirectory() as tmp:
            png = os.path.join(tmp, "out.png")
            for layer in ("6", "-1"):
                rc, _, err = _run(["visualize", "--case", case, "--layer", layer,
                                   "--out", png])
                self.assertEqual(rc, 2, layer)
                self.assertIn("out of range", err)
                self.assertFalse(os.path.exists(png))


if __name__ == "__main__":
    unittest.main()
