# cold_spt_lns - stress

Independent cold-start routes by [kesudh](https://github.com/kesudh).

Source, immutable source archives, runtime definition and reproduction commands
are in [the shared intro entry](../../intro/cold_spt_lns/README.md).

    python submissions/intro/cold_spt_lns/run.py --check-only --tier stress
    python submissions/intro/cold_spt_lns/run.py --reproduce --tier stress

Each case retains its actual producing run, full runtime, source version
and solver arguments in runs.json.

R8 is legal at delay 1,047,786, 50 below the preceding stress route. Its full original producing runtime is 8,123.75 seconds; its separate byte-exact replay took 8,679.922 seconds. The immutable source version is `65c3a988c30f56d3eab9910cf67f583e8982cf1e3350f8a5db065e2915cffcd6`, stored in this tier's reproducer archive. Source hashes, arguments and replay evidence are in [yielding_replay_proofs.json](yielding_replay_proofs.json). The producing toolchain was Rust/Cargo 1.98.1 on Windows. All 44 nonexempt routes passed the canonical originality comparison against 1,339 peer blobs. No tier lead is claimed.
