# cold_spt_lns - congested

Independent cold-start routes by [kesudh](https://github.com/kesudh).

Source, immutable source archives, runtime definition and reproduction commands
are in [the shared intro entry](../../intro/cold_spt_lns/README.md).

    python submissions/intro/cold_spt_lns/run.py --check-only --tier congested
    python submissions/intro/cold_spt_lns/run.py --reproduce --tier congested

Each case retains its actual producing run, full runtime, source version
and solver arguments in runs.json.
Full same-platform exact replay certificate for congested/case_04 (+242) is in [yielding_replay_proofs.json](yielding_replay_proofs.json). The original producer time remains in runtime.json; replay time is separate in the certificate.
Full same-platform exact replay certificate for congested/case_03 (+26) is in [yielding_replay_proofs.json](yielding_replay_proofs.json). The original producer time remains in runtime.json; replay time is separate in the certificate.
