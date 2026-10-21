# coordinated_refinement

By [jay-tau](https://github.com/jay-tau).

All 45 routes are legal in the captured snapshot. Relative to the strongest compared public entry in each tier, 4 tier aggregates improve and 2 tie. Only 6 cases strictly improve on the best audited public route for that case; the other 39 retain that delay. Combining public case winners can already beat one public entry; only reductions below that per-case portfolio count as new search gains. Changes from PR47 include credited upstream improvements.

Compared public snapshot: `2026-10-13T17:36:35.948409+00:00`. Selected snapshot: `2026-10-13T17:45:45.081794+00:00`.

Previous submitted entry: [PR #47 at cb435d0](https://github.com/partcleda/eda-3d-routing-challenge/commit/cb435d0a755975cac922e0cc6ab74d96b0849bee).

| Tier | Best public aggregate | Updated aggregate | New case gains | Runtime (s) |
|---|---:|---:|---:|---:|
| intro | 1.15711274 | 1.15711274 | 0/20 | 490.76 |
| hard | 1.41568676 | 1.41568676 | 0/9 | 271.32 |
| scale | 1.14038469 | 1.14040764 | 2/8 | 243.27 |
| stress | 1.09862550 | 1.09866134 | 1/1 | 34.64 |
| congested | 1.35910071 | 1.35911268 | 1/4 | 121.58 |
| designs | 1.47817534 | 1.47820687 | 2/3 | 90.99 |

The measured runs start from these public parents:

| Public parent | Author | Pinned commit |
|---|---|---|
| coordinated_refinement | jay-tau | [cb435d0](https://github.com/partcleda/eda-3d-routing-challenge/commit/cb435d0a755975cac922e0cc6ab74d96b0849bee) |
| warm_lns_refinement | kesudh | [9e0b7a0](https://github.com/partcleda/eda-3d-routing-challenge/commit/9e0b7a022ef79cd0264a472210cbc0e775d59b6a) |

Including inherited declarations, the upstream entries are cuda-have-been-shorter; drama3d-portfolio; leonid-popryho; pathfinder_lns; pathfinder_refinement; spt_lns; warm_lns_refinement; credited authors are James (IrwinJam); Leonid Popryho; Taz33m; Tazeem Mahashin; YJ Kim; kesudh. Each tier declares its contributing entries in `derived_from`. Per-case metadata preserves exact public route URLs, commits, delays, byte sizes and SHA-256 values, plus the original metadata hash, attribution and matching case ancestry. Inherited metadata is labeled as the source author's report, with matching case ancestry and available historical credits retained. It does not replace the current checker score.

Selected route methods: exact A* and bounded group refinement: 45.

The refinement baseline uses exact root-distance-seeded A*, compact or random equal-delay choices, and bounded conflict-based or sequential group rerouting. Configuration names do not establish which move caused an improvement.

Earlier public routes retain their documented methods and ancestry. No from-scratch routing or global optimality claim is made.

`runtime.json` reports the full `harness_wall_time_s` for each selected run. One complete measured run per case, from reading the instance and pinned public warm start through refinement, validation and output/metadata writing. Native routing uses one CPU thread; the batch runs up to four cases concurrently on Linux 7.2.9-2-cachyos. Compilation is completed before timing. Upstream public-route generation time is unknown and excluded; no hardware-normalized speed claim is made. Hardware: AMD Ryzen 9 8945HS (16 logical CPUs), 15632572 KiB RAM. Submitted route bytes and hashes equal the measured output bytes and hashes.

| Case | Previous submitted delay | Best public case | Submitted | Runtime (s) |
|---|---:|---:|---:|---|
| case_01 | 8,540 | 8,540 | 8,540 | 30.088 |
| case_02 | 12,794 | 12,794 | 12,794 | 30.097 |
| case_03 | 10,922 | 10,922 | 10,922 | 30.097 |
| case_04 | 12,819 | 12,819 | 12,819 | 30.104 |
| case_05 | 14,963 | 14,963 | 14,963 | 30.164 |
| case_06 | 19,304 | 19,304 | 19,304 | 30.170 |
| case_07 | 20,775 | 20,775 | 20,775 | 30.185 |
| case_08 | 20,932 | 20,932 | 20,932 | 30.181 |
| case_09 | 21,046 | 21,046 | 21,046 | 30.230 |
