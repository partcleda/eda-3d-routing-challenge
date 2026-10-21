# coordinated_refinement

By [jay-tau](https://github.com/jay-tau).

All 45 routes are legal in the captured snapshot. Relative to the strongest compared public entry in each tier, 1/6 tier aggregates improve and 5/6 tie. Only 1/45 cases strictly improve on the best audited public route for that case; the other 44 retain that delay. Combining public case winners can already beat one public entry; only reductions below that per-case portfolio count as new search gains. Changes from PR51 include credited upstream improvements.

Compared public snapshot: `2026-10-14T15:08:23.460699+00:00`. Selected snapshot: `2026-10-14T15:41:01.183766+00:00`.

Previous submitted entry: [PR #51 at afac5dd](https://github.com/partcleda/eda-3d-routing-challenge/commit/afac5dd0e74dee8d9b87c6dca2fe7d653bd75746).

| Tier | Best public aggregate | Updated aggregate | New case gains | Runtime (s) |
|---|---:|---:|---:|---:|
| intro | 1.15712696 | 1.15712696 | 0/20 | 490.53 |
| hard | 1.41568676 | 1.41568676 | 0/9 | 270.93 |
| scale | 1.14040764 | 1.14040764 | 0/8 | 242.33 |
| stress | 1.09866134 | 1.09866134 | 0/1 | 34.38 |
| congested | 1.35911268 | 1.35911268 | 0/4 | 121.48 |
| designs | 1.47820687 | 1.47830842 | 1/3 | 120.99 |

The measured runs start from these public parents:

| Public parent | Author | Pinned commit |
|---|---|---|
| coordinated_refinement | jay-tau | [afac5dd](https://github.com/partcleda/eda-3d-routing-challenge/commit/afac5dd0e74dee8d9b87c6dca2fe7d653bd75746) |
| warm_lns_refinement | kesudh | [ffde849](https://github.com/partcleda/eda-3d-routing-challenge/commit/ffde849c7e279d5aa810b19c0eee7b728f701bf4) |

Including inherited declarations, the upstream entries are cuda-have-been-shorter; drama3d-portfolio; leonid-popryho; pathfinder_lns; pathfinder_refinement; spt_lns; warm_lns_refinement; credited authors are James (IrwinJam); Leonid Popryho; Taz33m; Tazeem Mahashin; YJ Kim; kesudh. Each tier declares its contributing entries in `derived_from`. Per-case metadata preserves exact public route URLs, commits, delays, byte sizes and SHA-256 values, plus the original metadata hash, attribution and matching case ancestry. Inherited metadata is labeled as the source author's report, with matching case ancestry and available historical credits retained. It does not replace the current checker score.

Selected route methods: exact A* and bounded group refinement: 45.

The measured configurations use exact root-distance-seeded A*, compact or random equal-delay choices, and bounded conflict-based or sequential group rerouting. The pin-access variant also prefers paths away from adjacent foreign pins when physical delays tie during some single-net reroutes. Configuration names and improvement logs do not establish that this preference caused a gain.

Earlier public routes retain their documented methods and ancestry. No from-scratch routing or global optimality claim is made.

`runtime.json` reports the full `harness_wall_time_s` for each selected run. One complete measured run per case, from reading the instance and pinned public warm start through refinement, validation and output/metadata writing. Native routing uses one CPU thread; the batch runs up to four cases concurrently on Linux 7.2.9-2-cachyos. Compilation is completed before timing. Upstream public-route generation time is unknown and excluded; no hardware-normalized speed claim is made. Hardware: AMD Ryzen 9 8945HS (16 logical CPUs), 15632572 KiB RAM. Submitted route bytes and hashes equal the measured output bytes and hashes.

| Case | Previous submitted delay | Best public case | Submitted | Runtime (s) |
|---|---:|---:|---:|---|
| case_01 | 8,540 | 8,540 | 8,540 | 30.113 |
| case_02 | 12,794 | 12,794 | 12,794 | 30.099 |
| case_03 | 10,922 | 10,922 | 10,922 | 30.076 |
| case_04 | 12,819 | 12,819 | 12,819 | 30.083 |
| case_05 | 14,963 | 14,963 | 14,963 | 30.116 |
| case_06 | 19,304 | 19,304 | 19,304 | 30.115 |
| case_07 | 20,775 | 20,775 | 20,775 | 30.115 |
| case_08 | 20,932 | 20,932 | 20,932 | 30.099 |
| case_09 | 21,046 | 21,046 | 21,046 | 30.116 |
