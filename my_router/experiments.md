# Experiment log

## Setup
Python 3.13, macOS. From the repo root:

    python3 my_router/router.py --suite benchmarks --out-dir my_router/runs/<name>
    python3 -m m3d.cli score-suite --suite benchmarks --submission-dir my_router/runs/<name>

Built with AI assistance (Claude). Experiments 4 and 5 are my own changes.

## Results (intro tier, 20 cases)

| # | Change | Aggregate | Legal | Runtime | Notes |
|---|--------|----------:|:-----:|--------:|-------|
| 0 | Example router, unchanged | 0.9437 | 20/20 | — | Starting point. Grows each tree by attaching the nearest sink, which minimizes wire, not delay. |
| 1 | Shortest-path tree from the driver, smallest net first | 0.9850 | 20/20 | — | The objective sums driver-to-sink delay, so each sink should get its own shortest path. |
| 2 | Same, largest net first | 1.0240 | 20/20 | — | Ordering matters a lot: big nets need the cheap middle layers most. |
| 3 | + reroute each net with the others fixed, until no gain; best of two orders | 1.0337 | 20/20 | — | Can never make a net worse, since its old route is still available. |
| 4 | + rip up a net and its blockers, reroute, keep if total drops; each net's ideal route cached once | 1.0539 | 20/20 | 672s | Ideal routes ignore other wires and pins never move, so caching them halves runtime with identical results. |
| 5 | + third ordering: largest ideal delay first | 1.0568 | 20/20 | 1063s | Orders by what's actually scored. Small gain; improved cases 4, 5, 7 and others, none worse. |
| 6 | Scale rip-up time with case size: 0.4 s per net per ordering (was a flat 20 s) | 1.0611 | 20/20 | 1777 s | Confirms the rip-up step was time-starved on large cases: 13–20 improved most, 17 and 19 moved above baseline, small cases unchanged. All 20 cases now beat baseline. Runtime +67%. |
| 7 | Swap sinks_desc for conflict_desc: order by overlap with other nets' ideal routes | 1.0632 | 20/20 | 1827 s | Best score. Mixed per case: 6, 12, 17 improved a lot; 18 dropped (likely a sinks_desc win). No single ordering wins everywhere, so best-of-several is what helps. |

## What I learned
-For one thing, it is impossible for any one score to come out on top in all areas, so the aggregate was the only possible way to truly quantify anything. 
-I learned to not trust A.I. code in all situations, as the rip-up was heavily time-bloated to begin. 
-Very largely, your search mission dictates what your tree will end up looking like.
-Runtime had by far the most stark tradeoff results, and for every ordering (and therefore more potential to reduce overhead), severe consequences in runtime unfolded.


## Next steps
First, this will need to be run on the hard tier. I also wish to run all four of the orderings as they reduce overhead in their own ways and this will probably drastically increase runtime as a consequence. Every rip-up also needs to be reduced in cost as that eats a lot of time. An improvement on the Dijkstra using heavy optimization may also be necessary, but I personally do not have the background or mathematical knowledge to do that yet.