# cold_spt_lns - hard

Independent cold-start routes by [kesudh](https://github.com/kesudh).

Source, immutable source archives, runtime definition and reproduction commands
are in [the shared intro entry](../../intro/cold_spt_lns/README.md).

    python submissions/intro/cold_spt_lns/run.py --check-only --tier hard
    python submissions/intro/cold_spt_lns/run.py --reproduce --tier hard

Exact same-platform replay certificates for the yielding hard/case_01, hard/case_04, hard/case_06, hard/case_07 and hard/case_08 routes: [yielding_replay_proofs.json](yielding_replay_proofs.json).

Each case retains its actual producing run, full runtime, source version
and solver arguments in runs.json.
