# warm_lns_refinement — scale

By [kesudh](https://github.com/kesudh). Exact shortest-path-tree warm refinement of
published routes; per-case provenance is byte-verified in `meta.json`.

| legal | total delay | aggregate | runtime (s) |
|---:|---:|---:|---:|
| 8/8 | 531420 | 1.1404 | 242.47 |

## Attribution

Routes carried verbatim, with the ref the shipped bytes were taken from:

| entry | author | ref | commit | routes |
|---|---|---|---|---:|
| coordinated_refinement | jay-tau | pull/47 head cb435d0a7559 | `cb435d0a7559` | 3 |
| coordinated_refinement | jay-tau | pull/51 head afac5dd0e74d | `afac5dd0e74d` | 2 |

Warm starts: the 3 case(s) refined here were warm-started from the best
published route for that case, published by coordinated_refinement (jay-tau),
drama3d-portfolio / cuda-have-been-shorter (YJ Kim), leonid-popryho (Leonid
Popryho) or pathfinder_lns (Tazeem Mahashin).

Conflict-based search is adapted from Sharon et al. (see `algorithm_references`).
`runtime.json`, per case:

* **This entry's own routes:** the run of this router that produced the shipped file - 30 s
  search budget, 4 worker threads, chain-fast mode, the engine's own END_TO_END_SECONDS.
  stress/case_01 reports 44.77 s because the budget is checked only at the top of the trial
  loop and a 901-net trial can exceed it; congested/case_01 reports 31.617 s, the run behind
  its previous route plus a 1.55 s partition-crossover run (see the congested README).
* **Carried routes, producer runtime published (14 files):** the producing author's own
  runtime for that exact file, read from their `runtime.json` at the credited commit
  (their measurement on their hardware). `meta.json` names the source per case.
* **Carried routes, no producer runtime published (12 files: intro case_06-08, 12, 15-18;
  hard case_01-03, 08):** one run of this entry's router on the instance (30 s, 4 workers,
  chain-fast, seed 1), warm-started from the shipped file, which returns it unchanged. That
  run re-produces the route; it did not originate it, and `meta.json` says so per case.

Timed parallel search is nondeterministic, so a repeat can return a different legal route.
No global-optimality claim is made.
