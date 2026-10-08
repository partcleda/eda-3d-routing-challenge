# coordinated_refinement

By [jay-tau](https://github.com/jay-tau).

All 45 routes are legal in the captured snapshot. Relative to the strongest compared public entry in each tier, 5 tier aggregates improve and 1 tie. Only 11 cases strictly improve on the best audited public route for that case; the other 34 retain that delay. The larger changes from the previous merged PR29 snapshot also include other authors' improvements and are not attributed entirely to this refinement.

Compared public snapshot: `2026-10-05T13:46:53.920278+00:00`. Selected snapshot: `2026-10-05T14:48:49.120664+00:00`.

Previous merged entry: [PR #29 on main at 2566f5f](https://github.com/partcleda/eda-3d-routing-challenge/commit/2566f5f120392182dd97db77ba7760f8bb9b0828).

| Tier | Previous merged entry | Best public entry | Public aggregate | Updated aggregate | Public delay → updated | New case gains |
|---|---:|---|---:|---:|---:|---:|
| intro | 1.15657709 | warm_lns_refinement | 1.15669142 | 1.15670876 | 339,284 → 339,274 | 4/20 |
| hard | 1.41362123 | warm_lns_refinement | 1.41368829 | 1.41368829 | 142,313 → 142,313 | 0/9 |
| scale | 1.13986584 | warm_lns_refinement | 1.13989829 | 1.13991623 | 531,672 → 531,664 | 2/8 |
| stress | 1.09842523 | warm_lns_refinement | 1.09842945 | 1.09843367 | 1,042,310 → 1,042,306 | 1/1 |
| congested | 1.35077876 | warm_lns_refinement | 1.35112824 | 1.35115967 | 476,073 → 476,057 | 1/4 |
| designs | 1.46731109 | warm_lns_refinement | 1.46865417 | 1.46891586 | 204,279 → 204,245 | 3/3 |

The selected routes use these public parents, directly or through our recorded refinement steps:

| Public parent | Author | Pinned commit |
|---|---|---|
| coordinated_refinement | jay-tau | [2566f5f](https://github.com/partcleda/eda-3d-routing-challenge/commit/2566f5f120392182dd97db77ba7760f8bb9b0828) |
| warm_lns_refinement | kesudh | [91ebf3e](https://github.com/partcleda/eda-3d-routing-challenge/commit/91ebf3e500c018bab5c43cb415fd904c189333d0) |

Including inherited declarations, the upstream entries are drama3d-portfolio; leonid-popryho; pathfinder_lns; warm_lns_refinement; credited authors are Leonid Popryho; Taz33m; Tazeem Mahashin; YJ Kim; kesudh. Each tier declares its contributing entries in `derived_from`. Per-case metadata preserves exact public route URLs, commits, delays, byte sizes and SHA-256 values, plus the original metadata hash, attribution and matching case ancestry. Case ancestry is excerpted without execution commands, logs or machine-specific paths. Inherited metadata is the source author's report and may describe earlier refinement stages; it is not a current checker score or a fresh reproduction of their router. Available pinned historical metadata preserves earlier versions' upstream credit.

Selected route methods: exact A* and bounded group refinement: 11; retained public route: 34.

The refinement baseline uses exact root-distance-seeded A*, compact or random equal-delay choices, and bounded conflict-based or sequential group rerouting. Configuration names do not establish which move caused an improvement.

Earlier public routes retain their documented methods and ancestry. No from-scratch routing or global optimality claim is made.

Runtime files are omitted because measured refinement work excludes the compute that generated the public parent routes. No end-to-end runtime or Pareto claim is made. Route JSON is compactly serialized without changing parsed content; metadata distinguishes source-file hashes from submitted-file hashes.

| Case | Previous merged entry delay | Best public case | Submitted | Selected method |
|---|---:|---:|---:|---|
| case_01 | 8,542 | 8,542 | 8,542 | retained public route |
| case_02 | 12,804 | 12,804 | 12,804 | retained public route |
| case_03 | 10,924 | 10,924 | 10,924 | retained public route |
| case_04 | 12,861 | 12,861 | 12,861 | retained public route |
| case_05 | 14,987 | 14,985 | 14,985 | retained public route |
| case_06 | 19,322 | 19,320 | 19,320 | retained public route |
| case_07 | 20,887 | 20,887 | 20,887 | retained public route |
| case_08 | 20,932 | 20,932 | 20,932 | retained public route |
| case_09 | 21,062 | 21,058 | 21,058 | retained public route |
