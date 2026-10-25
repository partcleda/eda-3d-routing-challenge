# cold_spt_lns - designs

Independent cold-start routes by [kesudh](https://github.com/kesudh).

Source, immutable source archives, runtime definition and reproduction commands
are in [the shared intro entry](../../intro/cold_spt_lns/README.md).

    python submissions/intro/cold_spt_lns/run.py --check-only --tier designs
    python submissions/intro/cold_spt_lns/run.py --reproduce --tier designs

Each case retains its actual producing run, full runtime, source version
and solver arguments in runs.json.

The retained designs/ctrl route has delay **51,158** and is byte-identical to a fresh exact replay of archived genericRust source44 `44df9af614ff`. Its original producer runtime remains **15,379.797 seconds** in runs.json/runtime.json; replay time is recorded separately in the ROOT proof and is never substituted. This is 674 below the predecessor record. See [ctrl_exact_replay_proof.json](ctrl_exact_replay_proof.json).
The retained designs/router route has delay **70,329** and is byte-identical to the ROOT exact replay of archived source44 44df9af614ff. Its original producer runtime remains **25,239.515 seconds** in runs.json/runtime.json; the separate replay took **17,593.359 seconds** and is recorded only in its certificate. This is a 196 record difference from the predecessor. See [router_exact_replay_proof.json](router_exact_replay_proof.json) and [router_exact_replay.log](router_exact_replay.log).


The retained int2float route has delay **94,524**, produced from raw input by
archived source `6ead40b` in **58,925.310 seconds** on the Linux/Rust 1.90.0
GCE system recorded in runs.json. Its joint search adds internally generated
alternatives that avoid one neighboring net's preferred tree at a time. A full
same-platform replay reproduced the exact bytes; [replay_proof.json](replay_proof.json)
binds its source, arguments, hashes and separate replay timing. Other designs
cases retain their original Windows producing records. Exact-byte replay of
int2float uses the recorded Linux/Rust 1.90.0 environment.
