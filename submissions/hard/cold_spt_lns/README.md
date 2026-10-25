# cold_spt_lns - hard

Independent cold-start routes by [kesudh](https://github.com/kesudh).

Source, immutable source archives, runtime definition and reproduction commands
are in [the shared intro entry](../../intro/cold_spt_lns/README.md).

    python submissions/intro/cold_spt_lns/run.py --check-only --tier hard
    python submissions/intro/cold_spt_lns/run.py --reproduce --tier hard

Exact same-platform replay certificates for the yielding hard/case_01, hard/case_04, hard/case_06, hard/case_07 and hard/case_08 routes: [yielding_replay_proofs.json](yielding_replay_proofs.json).
hard/case_04 now uses the CBS-disjoint candidate (delay 13355, gain 4); its exact replay certificate is in [yielding_replay_proofs.json](yielding_replay_proofs.json). The original producer time remains authoritative; separate replay time is recorded in the certificate. Source archive: [61c897aa8f6e](../../intro/cold_spt_lns/reproducers/61c897aa8f6e5a3cf226092c72b37f2e65c48f565709c31916c8c8b048b40ddf/).

Each case retains its actual producing run, full runtime, source version
and solver arguments in runs.json.
