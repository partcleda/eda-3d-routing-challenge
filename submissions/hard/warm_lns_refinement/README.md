# warm_lns_refinement — hard

By [kesudh](https://github.com/kesudh). Exact shortest-path-tree warm refinement of
published routes; per-case provenance is byte-verified in `meta.json`.

| legal | total delay | aggregate | runtime (s) |
|---:|---:|---:|---:|
| 9/9 | 142095 | 1.4157 | 270.15 |

## Attribution

Routes carried verbatim, with the ref the shipped bytes were taken from:

| entry | author | ref | commit | routes |
|---|---|---|---|---:|
| coordinated_refinement | jay-tau | origin/main | `b13a8290d052` | 1 |
| cuda-have-been-shorter | YJ Kim | origin/main | `2d321cdb5e2f` | 3 |
| leonid-popryho | Leonid Popryho | pull/46 head 2d36fd1e | `2d36fd1ec432` | 5 |

Warm starts: the 0 case(s) refined here were warm-started from the best
published route for that case, published by coordinated_refinement (jay-tau),
drama3d-portfolio / cuda-have-been-shorter (YJ Kim), leonid-popryho (Leonid
Popryho) or pathfinder_lns (Tazeem Mahashin).

Conflict-based search is adapted from Sharon et al. (see `algorithm_references`).
`runtime.json` is one end-to-end run of this router per case at a uniform 30 s
search budget. Timed parallel search is nondeterministic, so a repeat can return a
different legal route. No global-optimality claim is made.
