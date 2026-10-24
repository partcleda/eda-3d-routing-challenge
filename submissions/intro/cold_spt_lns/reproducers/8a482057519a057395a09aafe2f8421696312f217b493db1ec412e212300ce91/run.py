#!/usr/bin/env python3
"""Generate or reproduce cold_spt_lns from instance JSON only (Python stdlib)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT))
from m3d.checker import check
from m3d.model import Instance, Submission

TIERS = ("intro", "hard", "scale", "congested", "designs")
SETTINGS = ["--seed", "20261022", "--starts", "2", "--rounds", "200",
            "--sweeps", "6", "--moves", "128"]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def instance_digest(path):
    # A Git checkout can use CRLF on Windows and LF on Linux. Hash the same
    # canonical instance text on both; preserve the actual input hash as well.
    return hashlib.sha256(Path(path).read_bytes().replace(b"\r\n", b"\n")).hexdigest()


def write_json(path, obj):
    Path(path).write_text(json.dumps(obj, indent=2) + "\n", encoding="utf-8")


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--tier", choices=TIERS, action="append")
    ap.add_argument("--case", help="Optional case name within the selected tier")
    ap.add_argument("--out-root", type=Path, help="Alternative submissions root")
    ap.add_argument("--reproduce", action="store_true", help="Rerun and require byte-identical outputs")
    ap.add_argument("--check-only", action="store_true")
    ap.add_argument("--cargo", default="cargo")
    ap.add_argument("--hardware", default=platform.processor() or platform.machine())
    args = ap.parse_args()
    if args.check_only and args.reproduce:
        ap.error("choose --check-only or --reproduce")
    crate = HERE / "router"
    source_hashes = {str(p.relative_to(HERE)).replace(os.sep, "/"): digest(p)
                     for p in (crate / "src/main.rs", crate / "Cargo.toml", crate / "Cargo.lock", HERE / "run.py")}
    if not args.check_only:
        subprocess.run([args.cargo, "build", "--release", "--locked", "--manifest-path", str(crate / "Cargo.toml")], check=True)
    exe = crate / "target/release" / ("cold-spt-lns.exe" if os.name == "nt" else "cold-spt-lns")
    for tier in args.tier or TIERS:
        suite = ROOT / ("benchmarks" if tier == "intro" else "benchmarks_" + tier)
        cases = json.loads((suite / "suite.json").read_text())["cases"]
        dest = (args.out_root or ROOT / "submissions") / tier / "cold_spt_lns"
        dest.mkdir(parents=True, exist_ok=True)
        runtime_path = dest / "runtime.json"
        records_path = dest / "runs.json"
        # Keep other cases when explicitly resuming a single case.
        runtimes = json.loads(runtime_path.read_text()) if args.case and runtime_path.exists() else {}
        records = json.loads(records_path.read_text()) if args.case and records_path.exists() else {}
        selected = [c for c in cases if not args.case or c["name"] == args.case]
        if not selected:
            raise ValueError("case not in suite: " + str(args.case))
        for c in selected:
            name = c["name"]
            original = suite / c["instance_file"]
            committed = dest / (name + ".sol.json")
            if args.check_only:
                result = check(Instance.load(str(original)), Submission.load(str(committed)))
                if not result.legal:
                    raise RuntimeError(str(result.to_dict()))
                recorded = json.loads(records_path.read_text())[name]
                assert recorded["input_sha256"] == instance_digest(original)
                assert recorded["output_sha256"] == digest(committed)
                assert recorded["source_sha256"] == source_hashes
                print(f"{tier}/{name}: legal, delay={result.total_delay}, hashes match", flush=True)
                continue
            with tempfile.TemporaryDirectory(prefix="cold-spt-") as tmp:
                tmp = Path(tmp)
                # Solver working directory contains only this case's instance.
                instance = tmp / "instance.json"
                output = tmp / "output.sol.json"
                shutil.copyfile(original, instance)
                command = [str(exe), str(instance), str(output)] + SETTINGS
                started = time.perf_counter()
                run = subprocess.run(command, cwd=tmp, capture_output=True, text=True)
                elapsed = time.perf_counter() - started
                if run.returncode:
                    raise RuntimeError(f"{tier}/{name} failed:\n{run.stderr}")
                result = check(Instance.load(str(instance)), Submission.load(str(output)))
                if not result.legal:
                    raise RuntimeError(f"{tier}/{name}: {result.to_dict()}")
                if args.reproduce:
                    if output.read_bytes() != committed.read_bytes():
                        raise RuntimeError(f"{tier}/{name}: reproduction differs")
                    print(f"{tier}/{name}: byte-identical, legal, delay={result.total_delay}, replay={elapsed:.3f}s", flush=True)
                    continue
                shutil.copyfile(output, committed)
                runtimes[name] = elapsed
                records[name] = {
                    "input_sha256": instance_digest(instance),
                    "input_file_sha256": digest(instance),
                    "input_hash_scope": "UTF-8 instance text with CRLF normalized to LF",
                    "output_sha256": digest(output),
                    "source_sha256": source_hashes, "binary_sha256": digest(exe),
                    "solver_args": SETTINGS, "total_delay": result.total_delay,
                    "wall_seconds": elapsed, "hardware": args.hardware,
                    "platform": platform.platform(), "threads": 1,
                    "solver_log": run.stderr.splitlines(),
                }
                write_json(runtime_path, runtimes)
                write_json(records_path, records)
                print(f"{tier}/{name}: legal, delay={result.total_delay}, runtime={elapsed:.3f}s", flush=True)


if __name__ == "__main__":
    main()
