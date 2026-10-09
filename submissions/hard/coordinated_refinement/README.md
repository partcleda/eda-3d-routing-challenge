# coordinated_refinement

By [jay-tau](https://github.com/jay-tau).

All 45 routes are legal in the captured snapshot. Relative to the strongest compared public entry in each tier, 5 tier aggregates improve and 1 tie. Only 11 cases strictly improve on the best audited public route for that case; the other 34 retain that delay. Combining public case winners can already beat one public entry; only reductions below that per-case portfolio count as new search gains. Changes from PR36 include credited upstream improvements.

Compared public snapshot: `2026-10-09T16:05:28.678814+00:00`. Selected snapshot: `2026-10-09T16:13:48.105262+00:00`.

Previous submitted entry: [PR #36 at 845e94d](https://github.com/partcleda/eda-3d-routing-challenge/commit/845e94d6c85adfbb5b5bb1c8773a52b28aefcad6).

| Tier | Best public aggregate | Updated aggregate | New case gains | Runtime (s) |
|---|---:|---:|---:|---:|
| intro | 1.15709340 | 1.15711274 | 3/20 | 490.27 |
| hard | 1.41568676 | 1.41568676 | 0/9 | 270.86 |
| scale | 1.14035091 | 1.14038034 | 3/8 | 242.54 |
| stress | 1.09859809 | 1.09860652 | 1/1 | 34.64 |
| congested | 1.35889586 | 1.35897734 | 2/4 | 121.77 |
| designs | 1.47790660 | 1.47802622 | 2/3 | 91.17 |

The measured runs start from these public parents:

| Public parent | Author | Pinned commit |
|---|---|---|
| coordinated_refinement | jay-tau | [845e94d](https://github.com/partcleda/eda-3d-routing-challenge/commit/845e94d6c85adfbb5b5bb1c8773a52b28aefcad6) |
| cuda-have-been-shorter | YJ Kim | [49b6443](https://github.com/partcleda/eda-3d-routing-challenge/commit/49b6443d9b599b77e09432c379ea7f422bdf4ffc) |
| pathfinder_refinement | Tazeem Mahashin | [2dda537](https://github.com/partcleda/eda-3d-routing-challenge/commit/2dda53752e68fc12796f8bffb5e37789098e5978) |
| warm_lns_refinement | kesudh | [74af725](https://github.com/partcleda/eda-3d-routing-challenge/commit/74af7256d17747a545c45e524d29e72e5801e755) |

Including inherited declarations, the upstream entries are cuda-have-been-shorter; drama3d-portfolio; leonid-popryho; pathfinder_lns; pathfinder_refinement; spt_lns; warm_lns_refinement; credited authors are James (IrwinJam); Leonid Popryho; Taz33m; Tazeem Mahashin; YJ Kim; kesudh. Each tier declares its contributing entries in `derived_from`. Per-case metadata preserves exact public route URLs, commits, delays, byte sizes and SHA-256 values, plus the original metadata hash, attribution and matching case ancestry. Inherited metadata is labeled as the source author's report, with matching case ancestry and available historical credits retained. It does not replace the current checker score.

Selected route methods: exact A* and bounded group refinement: 45.

The refinement baseline uses exact root-distance-seeded A*, compact or random equal-delay choices, and bounded conflict-based or sequential group rerouting. Configuration names do not establish which move caused an improvement.

Earlier public routes retain their documented methods and ancestry. No from-scratch routing or global optimality claim is made.

`runtime.json` reports the full `harness_wall_time_s` for each selected run. One complete measured run per case, from reading the instance and pinned public warm start through refinement, validation and output/metadata writing. Native routing uses one CPU thread; the batch runs up to four cases concurrently on Linux 7.2.9. Compilation is completed before timing. Upstream public-route generation time is unknown and excluded; no hardware-normalized speed claim is made. Hardware: AMD Ryzen 9 8945HS (16 logical CPUs), 15632568 KiB RAM. Submitted route bytes and hashes equal the measured output bytes and hashes.

| Case | Previous submitted delay | Best public case | Submitted | Runtime (s) |
|---|---:|---:|---:|---|
| case_01 | 8,542 | 8,540 | 8,540 | 30.080 |
| case_02 | 12,804 | 12,794 | 12,794 | 30.084 |
| case_03 | 10,924 | 10,922 | 10,922 | 30.085 |
| case_04 | 12,861 | 12,819 | 12,819 | 30.089 |
| case_05 | 14,985 | 14,963 | 14,963 | 30.092 |
| case_06 | 19,320 | 19,304 | 19,304 | 30.100 |
| case_07 | 20,887 | 20,775 | 20,775 | 30.107 |
| case_08 | 20,932 | 20,932 | 20,932 | 30.111 |
| case_09 | 21,058 | 21,046 | 21,046 | 30.116 |
