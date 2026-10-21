# Contributing a submission

The leaderboard is **PR-based**: you open a pull request that adds your routing
results under `submissions/`, and CI verifies them. Everything runs on the Python
standard library — no install needed.

**Use any language and any hardware.** A submission is just JSON route files, so
your router can be written in anything and run on a CPU, a GPU, a cluster, or a
Mac laptop — the toolkit only reads and scores your output. The checker/scorer are
pure Python 3.9+ (macOS, Linux, Windows), and scoring is deterministic across
platforms (exact integer arithmetic on the committed benchmarks), so your local
score matches CI's.

## 1. Pick a tier and produce routes

Choose a tier (`intro`, `hard`, `scale`, `stress`, `congested`, or `designs`) and
write a router that reads each case's `*.json` instance and emits a solution
`*.sol.json` (the format is in [`docs/FORMATS.md`](docs/FORMATS.md); see
`examples/example_submission.py` for a working starting point).

Your goal: for every case in the tier, output a **legal** routing tree for every
net that **minimizes total delay**. You beat the baseline by scoring a higher
aggregate (each case is normalized to that tier's baseline, so the baseline scores
1.0000 and higher is better).

## 2. Lay out your submission

```
submissions/<tier>/<your-name>/
    <case>.sol.json     one per case (name matches the instance, e.g. case_01.sol.json / ctrl.sol.json)
    runtime.json        required: {"<case>": seconds, ...}  (see "Runtime" below)
    meta.json           author / method / url / date  (copy submissions/_template/meta.json)
```

A submission is ranked only if it has a **legal solution for every case** in the
tier. Partial submissions are listed as incomplete.

## 3. Verify locally

```bash
# score your submission against the tier (uses the independent checker)
python -m m3d.cli score-suite --suite benchmarks_<tier> \
    --submission-dir submissions/<tier>/<your-name>

# regenerate the leaderboard so your entry appears
python -m m3d.cli leaderboard-all          # writes LEADERBOARD.md + the README block
```

(For the `intro` tier the suite dir is `benchmarks`, not `benchmarks_intro`.)

`make verify-submissions` runs the same submission checks CI runs (CI also runs
the unit tests, `python -m unittest discover -s tests -q`).

## 4. Open the PR

Commit **only** these paths and open a pull request:

* `submissions/<tier>/<your-name>/**`
* the regenerated `LEADERBOARD.md` and `README.md` (only its generated
  leaderboard block changes)

CI will:

1. reject the PR if it modifies anything outside `submissions/**`,
   `LEADERBOARD.md` and README.md's generated leaderboard block (benchmarks and
   the toolkit are off-limits — that keeps scores comparable);
2. re-check every submitted solution with `m3d/checker.py`;
3. confirm `LEADERBOARD.md` and the README block match a fresh `leaderboard-all` run.

Because the score is recomputed from your route files, you cannot fake a number —
you can only rank higher by submitting better legal routes.

## Rules

* Route files must be legal under `m3d/checker.py` (connected acyclic trees, no
  resource conflicts, no routing through another net's pin, legal moves only).
* Don't modify the benchmark instances, references, or the toolkit in a submission
  PR. Toolkit changes are welcome — as separate PRs.
* One directory per distinct method. Iterating on your own entry is fine.
* **Runtime (required).** `runtime.json` gives, for every case in the tier, the
  wall-clock seconds of the one run that produced that case's route file. Time
  the router end to end on that case (input to `.sol.json`), not a sum over
  seeds or a single stage. `meta.json` must also say what it ran on:
  `"hardware": "<CPU / GPU, RAM>"` and `"threads": <CPU threads used>`. CI
  rejects new or updated submissions without these, and entries without a full
  `runtime.json` are unranked.
* **Derivative entries.** If your routes start from another entry's published
  routes (a warm start or a refinement), say so in every tier's `meta.json`:
  `"derived_from": {"submission": "<their entry>", "author": "<their name>"}`.
  The leaderboard marks such entries with `†` and names the upstream entry.
  Derivative entries stay on the board but cannot win a tier. Undeclared reuse
  of another entry's routes is removed.
* **Copies.** Every route file must come from your own router's run. A route
  file identical to another entry's for the same case is a copy and is not
  accepted, declared in `derived_from` or not: that includes a per-case pick of
  the best published routes, and a refinement that left a case unchanged
  (leave that case to the upstream entry, or improve it). CI rejects copies
  (`scripts/check_copies.py`); cases with fewer than 10 nets are exempt, since
  independent routers often find the same routes there.
* **Verified column.** The maintainers mark an entry `reproduced` (in
  `verification.json`, which submission PRs cannot change) once they have re-run
  its router and reproduced its routes. Linking your router's code in `meta.json`
  (`url`) makes that possible.

## Winners

Each tier's winner is the highest-ranked entry that is

1. **independent**: no `derived_from`, and no routes taken from another entry;
2. **ranked**: legal on every case, with a full `runtime.json`;
3. **reproduced**: before a tier's winners are named, the maintainers re-run
   the routers of its top three independent entries and check they reproduce
   the submitted routes (the `verified` column). An entry whose router code is
   not available to the maintainers, publicly via `url` in `meta.json` or
   privately on request, cannot be reproduced and cannot win.

If an entry does not reproduce, the next independent entry moves up.
