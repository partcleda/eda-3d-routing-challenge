# coordinated_refinement

By [jay-tau](https://github.com/jay-tau).

All 45 routes are legal in the captured snapshot. Relative to the strongest compared public entry in each tier, 6 tier aggregates improve and 0 tie. Only 27 cases strictly improve on the best audited public route for that case; the other 18 retain that delay. The larger changes from the previous PR24 snapshot also include other authors' improvements and are not attributed entirely to this refinement.

Compared public snapshot: `2026-10-03T19:03:01.012628+00:00`. Selected snapshot: `2026-10-03T19:16:50.263917+00:00`.

Previous entry: [PR #24 at aaaafa1](https://github.com/partcleda/eda-3d-routing-challenge/commit/aaaafa163661f27eb09e878d87f0b98d1c482e33).

| Tier | Previous PR24 | Best public entry | Public aggregate | Updated aggregate | Public delay → updated | New case gains |
|---|---:|---|---:|---:|---:|---:|
| intro | 1.15169569 | leonid-popryho | 1.15626643 | 1.15657709 | 339,406 → 339,328 | 8/20 |
| hard | 1.38844885 | leonid-popryho | 1.41231731 | 1.41362123 | 142,413 → 142,321 | 4/9 |
| scale | 1.12812438 | leonid-popryho | 1.13969984 | 1.13986584 | 531,766 → 531,688 | 7/8 |
| stress | 1.09147416 | leonid-popryho | 1.09841680 | 1.09842523 | 1,042,322 → 1,042,314 | 1/1 |
| congested | 1.31132971 | leonid-popryho | 1.35052988 | 1.35077876 | 476,333 → 476,231 | 4/4 |
| designs | 1.43606004 | leonid-popryho | 1.46673439 | 1.46731109 | 204,581 → 204,497 | 3/3 |

The selected routes use these public parents, directly or through our recorded refinement steps:

| Public parent | Author | Pinned commit |
|---|---|---|
| coordinated_refinement | jay-tau | [aaaafa1](https://github.com/partcleda/eda-3d-routing-challenge/commit/aaaafa163661f27eb09e878d87f0b98d1c482e33) |
| drama3d-portfolio | YJ Kim | [1f3d020](https://github.com/partcleda/eda-3d-routing-challenge/commit/1f3d020c70068fb1198469033709fae48c53bf21) |
| leonid-popryho | Leonid Popryho | [9d9589c](https://github.com/partcleda/eda-3d-routing-challenge/commit/9d9589cd4e2e9e5fafcb67a2f698f6f7d4be64c9) |

Including inherited declarations, the upstream entries are drama3d-portfolio; leonid-popryho; pathfinder_lns; warm_lns_refinement; credited authors are Leonid Popryho; Taz33m; Tazeem Mahashin; YJ Kim; kesudh. Each tier declares its contributing entries in `derived_from`. Per-case metadata preserves exact public route URLs, commits, delays, byte sizes and SHA-256 values, plus the original metadata hash, attribution and matching case ancestry. Inherited metadata is identified as the source author's report; it is not a fresh reproduction of their router. Retained earlier versions of our entry preserve their upstream credit.

Selected route methods: exact A* and bounded group refinement: 27; retained public route: 18.

The refinement baseline uses exact root-distance-seeded A*, compact or random equal-delay choices, and bounded conflict-based or sequential group rerouting. Configuration names do not establish which move caused an improvement.

Earlier public routes retain their documented methods and ancestry. No from-scratch routing or global optimality claim is made.

Runtime files are omitted because measured refinement work excludes the compute that generated the public parent routes. No end-to-end runtime or Pareto claim is made. Route JSON is compactly serialized without changing parsed content; metadata distinguishes source-file hashes from submitted-file hashes.

| Case | Previous PR24 delay | Best public case | Submitted | Selected method |
|---|---:|---:|---:|---|
| case_01 | 8,666 | 8,542 | 8,542 | retained public route |
| case_02 | 13,140 | 12,804 | 12,804 | retained public route |
| case_03 | 11,062 | 10,924 | 10,924 | retained public route |
| case_04 | 13,033 | 12,863 | 12,861 | exact A* and bounded group refinement |
| case_05 | 15,207 | 14,987 | 14,987 | retained public route |
| case_06 | 19,778 | 19,322 | 19,322 | retained public route |
| case_07 | 21,283 | 20,889 | 20,887 | exact A* and bounded group refinement |
| case_08 | 21,424 | 20,934 | 20,932 | exact A* and bounded group refinement |
| case_09 | 21,394 | 21,072 | 21,062 | exact A* and bounded group refinement |
