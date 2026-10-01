#!/usr/bin/env python3
"""Merge several submission folders into one, keeping the best legal result per case.

Scoring is per case, and a solution file for one case does not depend on any
other case, so picking the lowest-delay LEGAL file for each case from several
runs gives a submission that is at least as good as every run.  Every candidate
is re-checked with the repo's independent checker; illegal or unreadable files
are ignored.

Usage (run from the repository root):
    python merge_best.py --suite benchmarks --out subs/intro_best \
        subs/mine subs/intro_night subs/intro_120

Then score it as usual:
    python -m m3d.cli score-suite --suite benchmarks --submission-dir subs/intro_best
"""
import argparse
import json
import math
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from m3d.checker import check                      # noqa: E402
from m3d.model import Instance, Submission        # noqa: E402


def main() -> int:
    ap = argparse.ArgumentParser(description="best-of-N merge of submission folders")
    ap.add_argument("--suite", required=True, help="tier directory, e.g. benchmarks_hard")
    ap.add_argument("--out", required=True, help="output folder to create")
    ap.add_argument("--author", default="")
    ap.add_argument("dirs", nargs="+", help="submission folders to merge")
    args = ap.parse_args()

    with open(os.path.join(args.suite, "suite.json"), encoding="utf-8") as fh:
        man = json.load(fh)
    os.makedirs(args.out, exist_ok=True)

    logs = []
    n_ok = 0
    names = [os.path.basename(os.path.normpath(d)) for d in args.dirs]
    print(f"{'case':10s} {'best':>9s} {'from':28s} " + " ".join(f"{n:>12s}" for n in names))
    for c in man["cases"]:
        inst = Instance.load(os.path.join(args.suite, c["instance_file"]))
        cands = []
        row = []
        for d in args.dirs:
            path = os.path.join(d, f"{inst.name}.sol.json")
            if not os.path.exists(path):
                row.append("-")
                continue
            try:
                res = check(inst, Submission.load(path))
            except Exception:
                row.append("unreadable")
                continue
            if res.legal and res.total_delay:
                cands.append((res.total_delay, d, path))
                row.append(str(res.total_delay))
            else:
                row.append("ILLEGAL")
        if not cands:
            print(f"{inst.name:10s} {'NONE':>9s} {'':28s} " + " ".join(f"{x:>12s}" for x in row))
            continue
        total, src, path = min(cands, key=lambda t: t[0])
        shutil.copyfile(path, os.path.join(args.out, f"{inst.name}.sol.json"))
        n_ok += 1
        logs.append(math.log(c["baseline_total"] / total))
        print(f"{inst.name:10s} {total:9d} {src:28s} " + " ".join(f"{x:>12s}" for x in row))

    with open(os.path.join(args.out, "meta.json"), "w", encoding="utf-8") as fh:
        json.dump({"author": args.author,
                   "description": "best-of merge of: " + ", ".join(args.dirs)},
                  fh, indent=1)

    n = len(man["cases"])
    if n_ok == n:
        print(f"\nmerged {n_ok}/{n} cases, AGGREGATE = {math.exp(sum(logs) / n):.4f}")
    else:
        print(f"\nmerged {n_ok}/{n} cases (INCOMPLETE: some cases have no legal file)")
    return 0 if n_ok == n else 1


if __name__ == "__main__":
    raise SystemExit(main())
