# cold_spt_lns - designs

Independent cold-start routes by [kesudh](https://github.com/kesudh).

Source, immutable source archives, runtime definition and reproduction commands
are in [the shared intro entry](../../intro/cold_spt_lns/README.md).

    python submissions/intro/cold_spt_lns/run.py --check-only --tier designs
    python submissions/intro/cold_spt_lns/run.py --reproduce --tier designs

Each case retains its actual producing run, full runtime, source version
and solver arguments in runs.json.

The retained int2float route has delay **94,524**, produced from raw input by
archived source `6ead40b` in **58,925.310 seconds** on the Linux/Rust 1.90.0
GCE system recorded in runs.json. Its joint search adds internally generated
alternatives that avoid one neighboring net's preferred tree at a time. A full
same-platform replay reproduced the exact bytes; [replay_proof.json](replay_proof.json)
binds its source, arguments, hashes and separate replay timing. Other designs
cases retain their original Windows producing records. Exact-byte replay of
int2float uses the recorded Linux/Rust 1.90.0 environment.
