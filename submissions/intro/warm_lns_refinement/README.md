# warm_lns_refinement — intro

By [kesudh](https://github.com/kesudh). Exact shortest-path-tree warm refinement of
published routes; per-case provenance is byte-verified in `meta.json`.

| legal | total delay | aggregate | runtime (s) |
|---:|---:|---:|---:|
| 20/20 | 339096 | 1.1571 | 600.65 |

## Attribution

Routes carried verbatim, with the ref the shipped bytes were taken from:

| entry | author | ref | commit | routes |
|---|---|---|---|---:|
| coordinated_refinement | jay-tau | origin/main | `b13a8290d052` | 3 |
| coordinated_refinement | jay-tau | pull/36 head 845e94d6 | `845e94d6c85a` | 4 |
| coordinated_refinement | jay-tau | pull/47 head cb435d0a | `cb435d0a7559` | 3 |
| cuda-have-been-shorter | YJ Kim | origin/main | `2d321cdb5e2f` | 1 |
| leonid-popryho | Leonid Popryho | pull/46 head 2d36fd1e | `2d36fd1ec432` | 1 |

Warm starts: the 8 case(s) refined here were warm-started from the best
published route for that case, published by coordinated_refinement (jay-tau),
drama3d-portfolio / cuda-have-been-shorter (YJ Kim), leonid-popryho (Leonid
Popryho) or pathfinder_lns (Tazeem Mahashin).

Conflict-based search is adapted from Sharon et al. (see `algorithm_references`).
`runtime.json` is one end-to-end run of this router per case at a uniform 30 s
search budget. Timed parallel search is nondeterministic, so a repeat can return a
different legal route. No global-optimality claim is made.
