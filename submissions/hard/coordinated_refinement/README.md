# coordinated_refinement

By [jay-tau](https://github.com/jay-tau).

All 45 routes are legal in the captured snapshot. Relative to the strongest compared public entry in each tier, 2/6 tier aggregates improve and 4/6 tie. Only 2/45 cases strictly improve on the best audited public route for that case; the other 43 retain that delay. Combining public case winners can already beat one public entry; only reductions below that per-case portfolio count as new search gains. Changes from PR54 include credited upstream improvements.

Compared public snapshot: `2026-10-16T06:18:38.258054+00:00`. Selected snapshot: `2026-10-16T06:35:48.015527+00:00`.

Previous submitted entry: [PR #54 at d6e9320](https://github.com/partcleda/eda-3d-routing-challenge/commit/d6e9320533205466f48af0495a3ca26158213588).

| Tier | Best public aggregate | Updated aggregate | New case gains | Runtime (s) |
|---|---:|---:|---:|---:|
| intro | 1.15712958 | 1.15712958 | 0/20 | 512.41 |
| hard | 1.41571130 | 1.41571130 | 0/9 | 271.87 |
| scale | 1.14042238 | 1.14042745 | 1/8 | 251.32 |
| stress | 1.09867189 | 1.09867189 | 0/1 | 42.76 |
| congested | 1.35911268 | 1.35912066 | 1/4 | 138.77 |
| designs | 1.47832868 | 1.47832868 | 0/3 | 93.72 |

The measured runs start from these public parents:

| Public parent | Author | Pinned commit |
|---|---|---|
| coordinated_refinement | jay-tau | [d6e9320](https://github.com/partcleda/eda-3d-routing-challenge/commit/d6e9320533205466f48af0495a3ca26158213588) |
| warm_lns_refinement | kesudh | [67cc009](https://github.com/partcleda/eda-3d-routing-challenge/commit/67cc009518f8ee8a90cb589d416edd5c36fafd50) |

Including inherited declarations, the upstream entries are cuda-have-been-shorter; drama3d-portfolio; leonid-popryho; pathfinder_lns; pathfinder_refinement; spt_lns; warm_lns_refinement; credited authors are James (IrwinJam); Leonid Popryho; Taz33m; Tazeem Mahashin; YJ Kim; kesudh. Each tier declares its contributing entries in `derived_from`. Per-case metadata preserves exact public route URLs, commits, delays, byte sizes and SHA-256 values, plus the original metadata hash, attribution and matching case ancestry. Inherited metadata is labeled as the source author's report, with matching case ancestry and available historical credits retained. It does not replace the current checker score.

Selected route methods: exact A* and bounded group refinement: 45.

The measured configurations use exact root-distance-seeded A*, compact or random equal-delay choices, and bounded conflict-based or sequential group rerouting.

Earlier public routes retain their documented methods and ancestry. No from-scratch routing or global optimality claim is made.

`runtime.json` reports the full `harness_wall_time_s` for each selected run. One complete measured run per case, from reading the instance and pinned public warm start through refinement, validation and output/metadata writing. Native routing uses one CPU thread; the batch runs up to four cases concurrently on Linux 7.2.9-2-cachyos. Compilation is completed before timing. Upstream public-route generation time is unknown and excluded; no hardware-normalized speed claim is made. Hardware: AMD Ryzen 9 8945HS (16 logical CPUs), 15632572 KiB RAM. Submitted route bytes and hashes equal the measured output bytes and hashes.

| Case | Previous submitted delay | Best public case | Submitted | Runtime (s) |
|---|---:|---:|---:|---|
| case_01 | 8,540 | 8,540 | 8,540 | 30.433 |
| case_02 | 12,794 | 12,794 | 12,794 | 30.257 |
| case_03 | 10,922 | 10,922 | 10,922 | 30.229 |
| case_04 | 12,819 | 12,817 | 12,817 | 30.232 |
| case_05 | 14,963 | 14,963 | 14,963 | 30.144 |
| case_06 | 19,304 | 19,304 | 19,304 | 30.105 |
| case_07 | 20,775 | 20,775 | 20,775 | 30.108 |
| case_08 | 20,932 | 20,932 | 20,932 | 30.109 |
| case_09 | 21,046 | 21,046 | 21,046 | 30.252 |
