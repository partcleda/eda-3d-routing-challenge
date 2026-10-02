PYTHON ?= python3
TIER  ?= intro
SUITE ?= $(if $(filter intro,$(TIER)),benchmarks,benchmarks_$(TIER))
CASE  ?= benchmarks/case_01.json
SUBS  ?= examples/submissions

# the case's reference solution (<suite>/reference/<case>.sol.json) and name
CASE_NAME = $(basename $(notdir $(CASE)))
CASE_REF  = $(dir $(CASE))reference/$(CASE_NAME).sol.json

.PHONY: generate baseline baseline-suite example score-example evaluate visualize test info gif gif-suite generate-all generate-stress generate-congested generate-designs leaderboard verify-submissions pareto

generate:   ## build TIER (default intro) into SUITE (default: that tier's dir)
	$(PYTHON) -m m3d.cli generate --tier $(TIER) --out $(SUITE)

baseline:
	$(PYTHON) -m m3d.cli baseline --case $(CASE) --out baseline.sol.json

baseline-suite:
	$(PYTHON) -m m3d.cli baseline-suite --suite $(SUITE) --out-dir /tmp/m3d-baseline

example:
	$(PYTHON) examples/example_submission.py --suite $(SUITE) --out-dir $(SUBS)

score-example:
	$(PYTHON) -m m3d.cli score-suite --suite $(SUITE) --submission-dir $(SUBS)

evaluate: baseline
	$(PYTHON) -m m3d.cli evaluate --case $(CASE) --sol baseline.sol.json --suite $(SUITE)

visualize:
	$(PYTHON) -m m3d.cli visualize --case $(CASE) --sol $(CASE_REF) --out $(CASE_NAME).png

info:
	$(PYTHON) -m m3d.cli info --case $(CASE)

test:
	$(PYTHON) -m unittest discover -s tests -t .

gif:
	$(PYTHON) -m m3d.cli animate --mode layers --case benchmarks/case_12.json --sol benchmarks/reference/case_12.sol.json --out docs/layer_sweep.gif

gif-suite:
	$(PYTHON) -m m3d.cli animate --mode suite --suite $(SUITE) --out docs/suite_sweep.gif

generate-all:   ## the three quick tiers (intro, hard, scale); the others have their own targets
	$(PYTHON) -m m3d.cli generate --tier intro
	$(PYTHON) -m m3d.cli generate --tier hard
	$(PYTHON) -m m3d.cli generate --tier scale

generate-stress:   ## ~30 min: one 530x530 case whose baseline takes ~30 minutes
	$(PYTHON) -m m3d.cli generate --tier stress

generate-congested:   ## ~23 min: 4 large contended cases (negotiated baseline)
	$(PYTHON) -m m3d.cli generate --tier congested

generate-designs:   ## ~9 min: 3 real EPFL circuits (resumable; re-run to finish)
	$(PYTHON) -m m3d.cli generate --tier designs

leaderboard:   ## rebuild LEADERBOARD.md from submissions/<tier>/*
	$(PYTHON) -m m3d.cli leaderboard-all

verify-submissions:   ## CI's submission checks (CI also runs the unit tests: make test)
	$(PYTHON) scripts/verify_submissions.py
	$(PYTHON) -m m3d.cli leaderboard-all --check

pareto:   ## runtime-vs-delay Pareto plot for the hard tier
	$(PYTHON) -m m3d.cli pareto --suite benchmarks_hard --submissions-root submissions/hard --out docs/pareto.png
