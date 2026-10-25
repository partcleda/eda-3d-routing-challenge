# cold_spt_lns - scale

Independent cold-start routes by [kesudh](https://github.com/kesudh).

Source, immutable source archives, runtime definition and reproduction commands
are in [the shared intro entry](../../intro/cold_spt_lns/README.md).

    python submissions/intro/cold_spt_lns/run.py --check-only --tier scale
    python submissions/intro/cold_spt_lns/run.py --reproduce --tier scale

Each case retains its actual producing run, full runtime, source version
and solver arguments in runs.json.
Full same-platform exact replay certificates for scale/case_03 (+116) and scale/case_07 (+10) are in [yielding_replay_proofs.json](yielding_replay_proofs.json); combined gain 126. Original producer times remain in runtime.json; replay times are separate in the certificates.
