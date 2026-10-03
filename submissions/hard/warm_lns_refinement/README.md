# warm_lns_refinement — hard

By [kesudh](https://github.com/kesudh). Incremental warm refinement of published
routes, with complete upstream credits and source hashes in `meta.json`.

This revision has 9/9 legal cases, total delay
144,379, and aggregate 1.395751428357. It improves
PR26 at `89ca44091e30d9e8f2e7e56e5eb7e69de9b8294e` by 10 total-delay units.
Warm starts from PR26's drama3d-portfolio are credited to YJ Kim where selected,
alongside the previous coordinated_refinement and pathfinder_lns sources.

`runtime.json` reports 42.526400 seconds across this tier. These
are measured **incremental refinement** times, including private ancestor
stages and independent checking. They exclude public warm-start generation,
compilation and the tuning campaign; they are not from-scratch routing times.
Hardware: i7-13700HX, Windows; worker counts are recorded per command. Timing
was measured alongside other workloads and is not normalized across authors.

The method uses exact radix/A* shortest-path trees with feasible per-sink
bounds, neutral and group moves, preserved-route and displacement-chain repair,
bounded excursions, CPU search portfolios, and two-parent minimum-cut crossover
following PR24. An opt-in circular bucket queue accelerates integer searches.
The new `cbs-mixed` mode alternates neutral group moves with bounded conflict
search over static vertex-disjoint trees. Its exact shortest-path lower bounds
prune group configurations that cannot improve the incumbent. This adapts the
conflict-splitting principle of [Sharon et al.](https://doi.org/10.1016/j.artint.2014.11.006)
to multi-sink routing; a node/time-limited run makes no global-optimality claim.
The opt-in `chain-fast` and `chain-entry-fast` modes specialize congestion pricing,
avoiding repeated whole-grid setup while preserving fixed-work route choices.
Our CUDA prototypes were benchmarked separately and did not generate these
refinements. PR26 credits its public warm starts to a GPU-accelerated engine.

The versioned Windows replay bundle, source, commands and full provenance are
in `submissions/intro/warm_lns_refinement/replay`. Timed parallel searches can
produce different legal routes when repeated. Current experimental detour and
pair-sweep operators are included in the latest source but did not improve the
selected hard-case routes. No global optimality or immunity to future tuning
is claimed.
