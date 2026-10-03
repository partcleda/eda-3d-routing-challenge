# Versioned refinement replay

Run from the challenge repository checkout. Python 3.9+ is required for the
checker; archived executables target Windows x86-64. Rust source and Cargo lock
files accompany each executable, indexed by its recorded SHA-256. The latest
source is in `reproducer/src`; historical seeded runs use their recorded version.

Check the committed routes without executing a solver:

```powershell
python submissions/intro/warm_lns_refinement/replay/reproduce.py --repo . --check-only
```

For a replay, obtain the `submissions` directory from PR24 commit
`58585d7b496d505dfa35f2426df4269974e583fa`. The repository used by `--repo` must
also contain the unchanged pathfinder_lns files from main commit
`182434de94d0deeeced10578500846fcc3a6acab` and the unchanged benchmark suites.
Then run, for example:

For tiers whose provenance includes `drama3d-portfolio`, also obtain PR26's
`submissions` directory at commit `89ca44091e30d9e8f2e7e56e5eb7e69de9b8294e`
and supply `--pr26 C:/path/to/pr26/submissions`.

```powershell
python submissions/intro/warm_lns_refinement/replay/reproduce.py --repo . --pr24 C:/path/to/pr24/submissions --pr26 C:/path/to/pr26/submissions --out C:/path/to/replay-output --tier hard --case case_02 --fixed-work --require-identical
```

`--fixed-work` substitutes recorded move counts for single-worker LNS stages.
Timed parallel stages remain nondeterministic; `--require-identical` reports any
file mismatch rather than hiding it. Replaying all cases can take substantially
longer than checking the saved outputs. Compilation and public parent generation
are excluded from the reported incremental runtimes. Every private parent stage
is charged once in the dependency graph, including both parents of crossover.

Paths in the manifest are normalized identifiers (`MAIN`, `PR24`, `PR26`, `EXPERIMENT`),
not machine-specific locations. The replay tool resolves actual public inputs
and archived binaries through its arguments. The per-version Windows binary
hash is checked before execution. A native build on another platform may yield
different timing or tie trajectories; matching hardware/toolchain matters.
