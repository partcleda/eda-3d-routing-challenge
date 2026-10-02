# coordinated_refinement

By [jay-tau](https://github.com/jay-tau).

All 45 routes are legal in the captured snapshot. Relative to the strongest compared public entry in each tier, 6 tier aggregates improve and 0 tie. Only 28 cases strictly improve on the best audited public route for that case; the other 17 retain that delay. The larger changes from PR5 also include other authors' intervening improvements and are not attributed entirely to this refinement.

Compared public snapshot: `2026-10-02T08:18:10.869020+00:00`. Selected snapshot: `2026-10-02T08:29:30.616004+00:00`.

| Tier | PR5 aggregate | Best compared public | Updated aggregate | Public delay → updated | New case gains |
|---|---:|---:|---:|---:|---:|
| intro | 1.14951946 | 1.15143095 | 1.15169569 | 341,744 → 341,616 | 11/20 |
| hard | 1.38576514 | 1.38841986 | 1.38844885 | 144,991 → 144,987 | 1/9 |
| scale | 1.12483800 | 1.12777360 | 1.12812438 | 537,416 → 537,264 | 8/8 |
| stress | 1.09062989 | 1.09144710 | 1.09147416 | 1,048,978 → 1,048,952 | 1/1 |
| congested | 1.30421310 | 1.31116013 | 1.31132971 | 490,459 → 490,391 | 4/4 |
| designs | 1.42569639 | 1.43555030 | 1.43606004 | 209,145 → 209,065 | 3/3 |

The public parents are [Taz33m's pathfinder_lns](https://github.com/partcleda/eda-3d-routing-challenge/pull/3), originating at [c2d9d9f](https://github.com/Taz33m/eda-3d-routing-challenge/commit/c2d9d9f583d2f332ebb84d22592ee511f6e56e33) and read from [main 182434d](https://github.com/partcleda/eda-3d-routing-challenge/tree/182434de94d0deeeced10578500846fcc3a6acab), plus [kesudh's warm_lns_refinement PR #21](https://github.com/partcleda/eda-3d-routing-challenge/pull/21) at `1957ee13cd4cdcffacbc7e55a497929bb562ef30` and the older merged warm_lns_refinement routes. Those older routes credit pathfinder_lns at `4e21227867ee1f8f72c9f4d9ad446e05c20fe452`. Our retained [PR5 routes](https://github.com/partcleda/eda-3d-routing-challenge/pull/5) in turn credit pathfinder_lns at `a5ef5e2406682473b99c6496a87a6279b64db9ba`. Per-case metadata pins contributing public route files, authors, commits, delays, byte sizes and SHA-256 values.

Minimum-cut crossover chooses one complete parent route for each net. Cross-parent resource conflicts become implications, and a maximum-weight closure minimizes delay over the fixed two-parent choices. Repeated crossover across several parents is a heuristic; it does not prove an optimum over the whole portfolio or the routing problem.

The refinement baseline uses exact root-distance-seeded A*, compact or random equal-delay choices, and bounded conflict-based or sequential group rerouting. Retained earlier routes may also include the previously documented negotiated-congestion refinement. No from-scratch routing or global optimality claim is made.

Runtime files are omitted because measured refinement work excludes the compute that generated the public parent routes. No end-to-end runtime or Pareto claim is made. Route JSON is compactly serialized without changing parsed content; metadata distinguishes source-file hashes from submitted-file hashes.

| Case | PR5 delay | Best public case | Submitted | Selected method |
|---|---:|---:|---:|---|
| case_01 | 8,668 | 8,666 | 8,666 | retained public route |
| case_02 | 13,158 | 13,140 | 13,140 | retained public route |
| case_03 | 11,072 | 11,062 | 11,062 | retained public route |
| case_04 | 13,075 | 13,033 | 13,033 | retained public route |
| case_05 | 15,211 | 15,207 | 15,207 | retained public route |
| case_06 | 19,810 | 19,778 | 19,778 | retained public route |
| case_07 | 21,361 | 21,287 | 21,283 | minimum-cut crossover |
| case_08 | 21,508 | 21,424 | 21,424 | retained public route |
| case_09 | 21,442 | 21,394 | 21,394 | retained public route |
