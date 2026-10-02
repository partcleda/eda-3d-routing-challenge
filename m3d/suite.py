"""The released benchmark tiers, each a fixed master seed + reproducible per-case
seed schedule and a size ladder.

* ``intro`` (dir ``benchmarks/``)      — 20 cases; the easy on-ramp. Sparse; the
  simple rip-up baseline routes every case with no congestion. Baseline = simple.
* ``hard``  (dir ``benchmarks_hard/``) — dense, contended cases. The cheap middle
  layers and the die layers are crowded with pins and nets, so legality itself
  needs a real router: the simple baseline fails or thrashes on most of them,
  while the negotiated-congestion router certifies a legal solution. Baseline =
  negotiated (a much stronger reference to beat).
* ``scale`` (dir ``benchmarks_scale/``) — large but sparser cases (up to
  156x156x6, hundreds of nets, ~770 pins) where runtime is a first-class factor.
  Baseline = simple (routes with ~no congestion, so it certifies quickly).
* ``stress`` (dir ``benchmarks_stress/``) — one giant, sparse 530x530x6 case
  (901 nets, 2,612 pins) whose simple baseline takes ~30 minutes; a runtime and
  scaling stress test. Baseline = simple.
* ``congested`` (dir ``benchmarks_congested/``) — four large, contended cases
  (64x64x6 .. 116x116x6, up to 302 nets); the congested counterpart of
  ``stress``. Baseline = negotiated.

(The ``designs`` tier, built from real netlists, lives in ``m3d.designs``.)

Building a tier writes ``<dir>/case_NN.json``, ``<dir>/reference/case_NN.sol.json``
(the verified reference = the tier's baseline output, kept separate from inputs),
and ``<dir>/suite.json`` recording seeds, parameters and the per-case baseline
total (the scoring normalization).
"""
from __future__ import annotations

import json
import os
import random
import time
from typing import Dict, List

from .generator import GenConfig, generate_feasible

MASTER_SEED = 20260923
DEFAULT_LAYERS = 6          # configurable; the final layer count can change here

TIERS = ("intro", "hard", "scale", "stress", "congested")
_TIER_DIR = {"intro": "benchmarks", "hard": "benchmarks_hard",
             "scale": "benchmarks_scale", "stress": "benchmarks_stress",
             "congested": "benchmarks_congested"}
_TIER_SALT = {"intro": 0, "hard": 101, "scale": 202, "stress": 303,
              "congested": 404}


def tier_dir(tier: str) -> str:
    return _TIER_DIR[tier]


def suite_configs(tier: str = "intro", layers: int = DEFAULT_LAYERS,
                  master_seed: int = MASTER_SEED) -> List[GenConfig]:
    rng = random.Random(master_seed + _TIER_SALT[tier])
    cfgs: List[GenConfig] = []
    if tier == "intro":
        n = 20
        for i in range(n):
            seed = rng.randrange(1, 2 ** 31 - 1)
            side = 16 + 4 * i                       # 16 .. 92
            cfgs.append(GenConfig(
                name=f"case_{i + 1:02d}", width=side, height=side, layers=layers,
                center_delay=1, layer_slope=1, via_delay=3,
                n_nets=6 + 7 * i, frac_cross=0.4,
                p_twopin=max(0.45, 0.70 - 0.015 * i), max_fanout=4 + i // 5,
                frac_local=0.5, cell_min=2, cell_max=4, pins_per_cell=3, cell_gap=1,
                seed=seed, master_seed=master_seed, max_attempts=16,
                router="baseline"))
    elif tier == "hard":
        n = 9
        for i in range(n):
            seed = rng.randrange(1, 2 ** 31 - 1)
            side = 24 + 2 * i                       # 24 .. 40
            cfgs.append(GenConfig(
                name=f"case_{i + 1:02d}", width=side, height=side, layers=layers,
                center_delay=1, layer_slope=1, via_delay=3,
                n_nets=round(2.6 * side), frac_cross=0.45, p_twopin=0.6,
                max_fanout=6, frac_local=0.12, cell_min=2, cell_max=2,
                pins_per_cell=2, cell_gap=0,
                seed=seed, master_seed=master_seed, max_attempts=10,
                router="negotiated"))
    elif tier == "scale":
        n = 8
        for i in range(n):
            seed = rng.randrange(1, 2 ** 31 - 1)
            side = 100 + 8 * i                      # 100 .. 156
            cfgs.append(GenConfig(
                name=f"case_{i + 1:02d}", width=side, height=side, layers=layers,
                center_delay=1, layer_slope=1, via_delay=3,
                n_nets=round(1.7 * side), frac_cross=0.4, p_twopin=0.65,
                max_fanout=5, frac_local=0.5, cell_min=2, cell_max=3,
                pins_per_cell=3, cell_gap=1,
                seed=seed, master_seed=master_seed, max_attempts=16,
                router="baseline"))
    elif tier == "stress":
        # One giant, sparse case sized so the simple baseline takes ~30 minutes.
        # Route time fits time ~ 7e-6 * n_nets * side^2 (verified to side 240);
        # 530x530x6 with ~900 nets predicts ~1770 s. Sparse (rip-ups ~ 0), so the
        # baseline certifies it in a single pass.
        n = 1
        for i in range(n):
            seed = rng.randrange(1, 2 ** 31 - 1)
            side = 530
            cfgs.append(GenConfig(
                name=f"case_{i + 1:02d}", width=side, height=side, layers=layers,
                center_delay=1, layer_slope=1, via_delay=3,
                n_nets=901, frac_cross=0.4, p_twopin=0.65, max_fanout=5,
                frac_local=0.5, cell_min=2, cell_max=3, pins_per_cell=3, cell_gap=1,
                seed=seed, master_seed=master_seed, max_attempts=3,
                router="baseline"))
    elif tier == "congested":
        # Large AND contended: the congested counterpart of the sparse 'stress'
        # case. Net density (~2.6*side) and locality (0.12) keep the cheap middle
        # layers ~35% utilized at every size, so the simple rip-up baseline is
        # hopeless and the negotiated router is stressed by both size and
        # contention (baseline ~70 s / ~3.5 min / ~8 min / ~10.5 min across
        # the four cases).
        for side in (64, 88, 112, 116):
            seed = rng.randrange(1, 2 ** 31 - 1)
            i = len(cfgs)
            cfgs.append(GenConfig(
                name=f"case_{i + 1:02d}", width=side, height=side, layers=layers,
                center_delay=1, layer_slope=1, via_delay=3,
                n_nets=round(2.6 * side), frac_cross=0.45, p_twopin=0.6,
                max_fanout=6, frac_local=0.12, cell_min=2, cell_max=2,
                pins_per_cell=2, cell_gap=0,
                seed=seed, master_seed=master_seed, max_attempts=6,
                router="negotiated"))
    else:
        raise ValueError(f"unknown tier {tier!r}; choose from {TIERS}")
    return cfgs


def build_suite(out_dir: str, tier: str = "intro", layers: int = DEFAULT_LAYERS,
                master_seed: int = MASTER_SEED, verbose: bool = True) -> Dict:
    ref_dir = os.path.join(out_dir, "reference")
    os.makedirs(ref_dir, exist_ok=True)
    cfgs = suite_configs(tier=tier, layers=layers, master_seed=master_seed)
    manifest = {
        "format": "m3d-suite",
        "tier": tier,
        "master_seed": master_seed,
        "layers": layers,
        "n_cases": len(cfgs),
        "cases": [],
    }
    for cfg in cfgs:
        t0 = time.time()
        result = generate_feasible(cfg)
        dt = time.time() - t0
        inst = result.instance
        inst_file = f"{cfg.name}.json"
        ref_file = os.path.join("reference", f"{cfg.name}.sol.json")
        inst.save(os.path.join(out_dir, inst_file))
        result.reference.save(os.path.join(out_dir, ref_file))
        entry = {
            "name": cfg.name, "seed": inst.seed,
            "width": inst.width, "height": inst.height, "layers": inst.layers,
            "n_cells": len(inst.cells), "n_pins": len(inst.pins),
            "n_nets": len(inst.nets), "baseline_router": cfg.router,
            "baseline_total": result.baseline_total,
            "gen_attempts": result.attempts, "gen_seconds": round(dt, 2),
            "instance_file": inst_file, "reference_file": ref_file,
        }
        manifest["cases"].append(entry)
        if verbose:
            print(f"  [{tier}] {cfg.name}: {inst.width}x{inst.height}x{inst.layers}, "
                  f"{len(inst.nets)} nets, {len(inst.pins)} pins, "
                  f"baseline({cfg.router})={result.baseline_total}, "
                  f"attempts={result.attempts}, {dt:.1f}s", flush=True)
    with open(os.path.join(out_dir, "suite.json"), "w") as fh:
        json.dump(manifest, fh, indent=1)
    return manifest


def load_manifest(out_dir: str) -> Dict:
    with open(os.path.join(out_dir, "suite.json")) as fh:
        return json.load(fh)
