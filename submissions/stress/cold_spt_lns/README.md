# cold_spt_lns - stress

Independent cold-start routes by [kesudh](https://github.com/kesudh).

Source, immutable source archives, runtime definition and reproduction commands
are in [the shared intro entry](../../intro/cold_spt_lns/README.md).

    python submissions/intro/cold_spt_lns/run.py --check-only --tier stress
    python submissions/intro/cold_spt_lns/run.py --reproduce --tier stress

Each case retains its actual producing run, full runtime, source version
and solver arguments in runs.json.

The stress entry now retains source029 at checked delay 1,047,864, improving the prior own published delay 1,049,200 by 1,336. Its original complete producing time is 11,709.608999999939 seconds; exact replay time is kept separate. Exact Rust toolchain is unknown in the frozen metadata. Confirmed tier wins: 0/6; this change does not claim a tier lead.


The source029 update preserves the other 44 case records, replay certificates, and source archives. This update establishes the checked legal strict-gain result for case_01 and reports confirmed tier wins as 0/6; it makes no six-tier lead claim.
