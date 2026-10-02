"""Command-line interface: ``python -m m3d.cli <command> ...``

Commands
--------
  generate        build a deterministic benchmark tier (or all of them)
  baseline        run the baseline router on one case
  baseline-suite  run each case's baseline router (per suite.json) on a suite
  evaluate        check + score one submission against one case
  score-suite     check + score a full submission for one tier (leaderboard)
  visualize       render a case (and optional submission) to PNG
  info            print a short summary of a case
"""
from __future__ import annotations

import argparse
import json
import math
import os
import sys
import time
from typing import Dict, List, Optional

from .baseline import route
from .checker import check
from .model import Instance, Submission
from .scorer import CaseScore, leaderboard, score_case


def _load_manifest(suite_dir: str) -> Dict:
    with open(os.path.join(suite_dir, "suite.json")) as fh:
        return json.load(fh)


def _baseline_total_for(suite_dir: Optional[str], case_name: str,
                        explicit: Optional[int]) -> Optional[int]:
    if explicit is not None:
        return explicit
    if suite_dir:
        man = _load_manifest(suite_dir)
        for c in man["cases"]:
            if c["name"] == case_name:
                return c["baseline_total"]
    return None


# --------------------------------------------------------------------------- #
def cmd_generate(args: argparse.Namespace) -> int:
    from .suite import build_suite, tier_dir, TIERS
    from .designs import build_design_suite, DESIGN_DIR
    all_tiers = list(TIERS) + ["designs"]
    tiers = all_tiers if args.tier == "all" else [args.tier]
    for tier in tiers:
        if tier == "designs":
            out = args.out if (args.out and args.tier != "all") else DESIGN_DIR
            os.makedirs(out, exist_ok=True)
            print(f"generating tier 'designs' into {out} "
                  f"(real EPFL netlists, layers={args.layers}) ...")
            man = build_design_suite(out, layers=args.layers,
                                     master_seed=args.master_seed)
        else:
            out = args.out if (args.out and args.tier != "all") else tier_dir(tier)
            os.makedirs(out, exist_ok=True)
            print(f"generating tier '{tier}' into {out} "
                  f"(layers={args.layers}, master_seed={args.master_seed}) ...")
            man = build_suite(out, tier=tier, layers=args.layers,
                              master_seed=args.master_seed)
        print(f"done tier '{tier}': {len(man['cases'])} cases; "
              f"manifest at {os.path.join(out, 'suite.json')}")
    return 0


def cmd_import_design(args: argparse.Namespace) -> int:
    from .designs import load_netlist, generate_design_feasible, DesignConfig
    from .checker import check
    nl = load_netlist(args.blif)
    cfg = DesignConfig(name=args.name, layers=args.layers, channel=args.channel,
                       router=args.router)
    print(f"importing {args.blif} as '{args.name}' "
          f"(router={args.router}, channel={args.channel}) ...")
    res = generate_design_feasible(args.name, nl, cfg)
    inst = res.instance
    chk = check(inst, res.reference)
    inst.save(args.out)
    print(f"wrote {args.out}: {inst.width}x{inst.height}x{inst.layers}, "
          f"{len(inst.cells)} cells, {len(inst.pins)} pins, {len(inst.nets)} nets; "
          f"baseline={res.baseline_total}, legal={chk.legal}, attempts={res.attempts}")
    if args.reference:
        os.makedirs(os.path.dirname(os.path.abspath(args.reference)), exist_ok=True)
        res.reference.save(args.reference)
        print(f"wrote reference solution {args.reference}")
    return 0


def cmd_baseline(args: argparse.Namespace) -> int:
    inst = Instance.load(args.case)
    t0 = time.time()
    sub, stats = route(inst, order=args.order, rip_limit=args.rip_limit,
                       ops_factor=args.ops_factor)
    dt = time.time() - t0
    if sub is None:
        print(f"baseline FAILED to route {inst.name}: "
              f"routed {stats.routed_nets}/{stats.total_nets} nets, "
              f"rip-ups={stats.rip_ups}, ops={stats.ops}", file=sys.stderr)
        return 2
    res = check(inst, sub)
    out = args.out or (os.path.splitext(args.case)[0] + ".baseline.sol.json")
    sub.save(out)
    print(f"{inst.name}: legal={res.legal} total_delay={res.total_delay} "
          f"nets={stats.total_nets} rip_ups={stats.rip_ups} time={dt:.2f}s")
    print(f"wrote submission -> {out}")
    return 0 if res.legal else 3


def _route_case_baseline(inst: Instance, router: str, order: str):
    """Route with the router a manifest names as the case's baseline
    ('negotiated' for the contended tiers, else the simple rip-up router).
    Returns (submission or None, short failure detail)."""
    if router == "negotiated":
        from .negotiated import route_negotiated
        sub, st = route_negotiated(inst, order=order)
        return sub, f"{st.overused} overused after {st.iterations} iterations"
    sub, st = route(inst, order=order)
    return sub, f"{st.routed_nets}/{st.total_nets}"


def cmd_baseline_suite(args: argparse.Namespace) -> int:
    man = _load_manifest(args.suite)
    os.makedirs(args.out_dir, exist_ok=True)
    ok = True
    for c in man["cases"]:
        inst = Instance.load(os.path.join(args.suite, c["instance_file"]))
        router = c.get("baseline_router", "baseline")
        t0 = time.time()
        sub, detail = _route_case_baseline(inst, router, args.order)
        dt = time.time() - t0
        if sub is None:
            print(f"  {inst.name}: FAILED ({router}: {detail})")
            ok = False
            continue
        res = check(inst, sub)
        out = os.path.join(args.out_dir, f"{inst.name}.sol.json")
        sub.save(out)
        print(f"  {inst.name}: legal={res.legal} total={res.total_delay} "
              f"{router} time={dt:.2f}s")
        ok = ok and res.legal
    return 0 if ok else 3


def cmd_evaluate(args: argparse.Namespace) -> int:
    inst = Instance.load(args.case)
    try:
        sub = Submission.load(args.sol)
    except ValueError as exc:
        print(f"{inst.name}: unreadable submission {args.sol}: {exc}")
        return 2
    res = check(inst, sub)
    baseline_total = _baseline_total_for(args.suite, inst.name, args.baseline)
    print(f"{inst.name}: legal={res.legal} total_delay={res.total_delay}")
    if not res.legal:
        for r in res.reasons[:10]:
            print(f"  ! {r}")
        for n in res.nets:
            if not n.legal:
                print(f"  ! net {n.net}: {'; '.join(n.reasons) or 'illegal'}")
    if baseline_total is not None and res.legal and res.total_delay:
        cs = score_case(inst, sub, baseline_total)
        print(f"  baseline={baseline_total} ratio={cs.ratio:.4f} "
              f"(>1 beats baseline)")
    if args.out:
        with open(args.out, "w") as fh:
            json.dump(res.to_dict(), fh, indent=1)
        print(f"wrote result -> {args.out}")
    return 0 if res.legal else 1


def cmd_score_suite(args: argparse.Namespace) -> int:
    man = _load_manifest(args.suite)
    runtimes: Dict[str, float] = {}
    if args.runtimes:
        runtimes = _read_runtimes(args.runtimes)
    scores = []
    for c in man["cases"]:
        inst = Instance.load(os.path.join(args.suite, c["instance_file"]))
        sol_path = os.path.join(args.submission_dir, f"{inst.name}.sol.json")
        if not os.path.exists(sol_path):
            scores.append(CaseScore(inst.name, False, None, c["baseline_total"],
                                    None, runtimes.get(inst.name),
                                    ["submission file missing"]))
            continue
        try:
            sub = Submission.load(sol_path)
        except ValueError as exc:
            scores.append(CaseScore(inst.name, False, None, c["baseline_total"],
                                    None, runtimes.get(inst.name),
                                    [f"unreadable submission: {exc}"]))
            continue
        scores.append(score_case(inst, sub, c["baseline_total"],
                                 runtimes.get(inst.name)))
    lb = leaderboard(scores)
    for cs in lb.cases:
        flag = "OK " if cs.legal else "BAD"
        ratio = f"{cs.ratio:.4f}" if cs.ratio else "  -   "
        print(f"  [{flag}] {cs.case}: total={cs.total_delay} "
              f"baseline={cs.baseline_delay} ratio={ratio}")
    print(f"complete={lb.complete}  legal={lb.n_legal}/{lb.n_cases}  "
          f"AGGREGATE={lb.aggregate_score:.4f}")
    print(f"formula: {lb.formula}")
    if args.out:
        with open(args.out, "w") as fh:
            json.dump(lb.to_dict(), fh, indent=1)
        print(f"wrote leaderboard -> {args.out}")
    return 0 if lb.complete else 1


def cmd_visualize(args: argparse.Namespace) -> int:
    from .viz import visualize
    inst = Instance.load(args.case)
    if args.layer is not None and not 0 <= args.layer < inst.layers:
        print(f"m3d visualize: error: --layer {args.layer} is out of range for "
              f"{inst.name} (layers 0..{inst.layers - 1})", file=sys.stderr)
        return 2
    sub = Submission.load(args.sol) if args.sol else None
    out = args.out or (os.path.splitext(args.case)[0] +
                       (f".layer{args.layer}" if args.layer is not None else "") + ".png")
    visualize(inst, sub, layer=args.layer, out_path=out, show=args.show)
    print(f"wrote visualization -> {out}")
    return 0


def cmd_run_suite(args: argparse.Namespace) -> int:
    man = _load_manifest(args.suite)
    os.makedirs(args.out_dir, exist_ok=True)
    runtimes: Dict[str, float] = {}
    ok = True
    written = 0
    for c in man["cases"]:
        inst = Instance.load(os.path.join(args.suite, c["instance_file"]))
        t0 = time.time()
        if args.router == "negotiated":
            from .negotiated import route_negotiated
            sub, _ = route_negotiated(inst)
        elif args.router == "negotiated_fast":
            from .negotiated import route_negotiated
            sub, _ = route_negotiated(inst, max_iters=25, pres_mult=2.3, order="id")
        elif args.router == "negotiated2":
            # best-of-two-orders: more effort (~2x runtime) for lower delay
            from .negotiated import route_negotiated
            best = None
            for order in ("bbox_desc", "id"):
                s, _ = route_negotiated(inst, order=order)
                if s is None:
                    continue
                r = check(inst, s)
                if r.legal and (best is None or r.total_delay < best[1]):
                    best = (s, r.total_delay)
            sub = best[0] if best else None
        else:
            sub, _ = route(inst)
        dt = time.time() - t0
        runtimes[inst.name] = round(dt, 3)
        if sub is None:
            print(f"  {inst.name}: FAILED ({args.router}) {dt:.2f}s")
            ok = False
            continue
        res = check(inst, sub)
        sub.save(os.path.join(args.out_dir, f"{inst.name}.sol.json"))
        written += 1
        print(f"  {inst.name}: legal={res.legal} total={res.total_delay} "
              f"{args.router} {dt:.2f}s")
        ok = ok and res.legal
    with open(os.path.join(args.out_dir, "runtime.json"), "w") as fh:
        json.dump(runtimes, fh, indent=1)
    print(f"wrote {written}/{len(man['cases'])} solutions + runtime.json "
          f"-> {args.out_dir}")
    return 0 if ok else 3


def _read_runtimes(path: str) -> Dict[str, float]:
    """Read a {case: seconds} file, keeping only finite, non-negative numbers."""
    try:
        with open(path, encoding="utf-8-sig") as fh:
            data = json.load(fh)
    except (OSError, ValueError):
        return {}
    if not isinstance(data, dict):
        return {}
    return {k: float(v) for k, v in data.items()
            if isinstance(v, (int, float)) and not isinstance(v, bool)
            and math.isfinite(v) and v >= 0}


def _load_runtimes(submission_dir: str) -> Dict[str, float]:
    path = os.path.join(submission_dir, "runtime.json")
    return _read_runtimes(path) if os.path.exists(path) else {}


def _submission_entries(root: str):
    for name in sorted(os.listdir(root)):
        d = os.path.join(root, name)
        if os.path.isdir(d):
            yield name, d


def cmd_leaderboard(args: argparse.Namespace) -> int:
    from .scorer import score_submission_set, rank_submissions, pareto_frontier
    man = _load_manifest(args.suite)
    subs = [score_submission_set(man, args.suite, d, name, _load_runtimes(d))
            for name, d in _submission_entries(args.submissions_root)]
    ranked = rank_submissions(subs)
    frontier = set(pareto_frontier(subs))
    print(f"leaderboard for suite '{man.get('tier', args.suite)}' "
          f"({man['n_cases']} cases):")
    print(f"  {'rank':>4}  {'name':<16} {'agg':>7}  {'legal':>7}  "
          f"{'tot.delay':>10}  {'runtime(s)':>10}  pareto")
    for i, s in enumerate(ranked, 1):
        agg = f"{s.aggregate:.4f}" if s.complete else "  -   "
        td = str(s.total_delay) if s.total_delay is not None else "-"
        rt = f"{s.total_runtime:.2f}" if s.total_runtime is not None else "-"
        star = "*" if s.name in frontier else ""
        print(f"  {i:>4}  {s.name:<16} {agg:>7}  {s.n_legal:>3}/{s.n_cases:<3}  "
              f"{td:>10}  {rt:>10}  {star}")
    out = {
        "format": "m3d-leaderboard-multi",
        "suite": man.get("tier", args.suite),
        "n_cases": man["n_cases"],
        "pareto_frontier": sorted(frontier),
        "ranking": [s.to_dict() for s in ranked],
    }
    if args.out:
        json.dump(out, open(args.out, "w"), indent=1)
        print(f"wrote leaderboard -> {args.out}")
    if args.md:
        with open(args.md, "w", encoding="utf-8") as fh:
            fh.write(f"# Leaderboard — {man.get('tier', args.suite)} "
                     f"({man['n_cases']} cases)\n\n")
            fh.write("| rank | submission | aggregate | legal | total delay | runtime (s) | on Pareto |\n")
            fh.write("|---:|---|---:|:---:|---:|---:|:---:|\n")
            for i, s in enumerate(ranked, 1):
                agg = f"{s.aggregate:.4f}" if s.complete else "—"
                td = s.total_delay if s.total_delay is not None else "—"
                rt = f"{s.total_runtime:.2f}" if s.total_runtime is not None else "—"
                fh.write(f"| {i} | {s.name} | {agg} | {s.n_legal}/{s.n_cases} | "
                         f"{td} | {rt} | {'✓' if s.name in frontier else ''} |\n")
        print(f"wrote markdown leaderboard -> {args.md}")
    return 0


def _tier_dir_map() -> "dict":
    """tier name -> its suite directory, across all tiers (generated + designs)."""
    from .suite import TIERS, tier_dir
    from .designs import DESIGN_DIR
    m = {t: tier_dir(t) for t in TIERS}
    m["designs"] = DESIGN_DIR
    return m


def _load_meta(submission_dir: str) -> dict:
    path = os.path.join(submission_dir, "meta.json")
    if os.path.exists(path):
        try:
            with open(path, encoding="utf-8-sig") as fh:
                meta = json.load(fh)
        except (OSError, ValueError):
            return {}
        return meta if isinstance(meta, dict) else {}
    return {}


def _md_cell(value) -> str:
    """One Markdown table cell: collapse whitespace/newlines and escape '|'."""
    return " ".join(str(value).split()).replace("|", "\\|")


def _derived_from(meta: dict):
    """The upstream entry a submission builds on, or None.

    Declared in meta.json as `derived_from` -- either the upstream submission's
    name or an object with `submission` (and optionally `author`). `warm_start`
    is accepted as an alias. Returns a display string like "x (author)"."""
    src = meta.get("derived_from") or meta.get("warm_start")
    if not src:
        return None
    if isinstance(src, dict):
        name = src.get("submission") or src.get("name") or "another entry"
        author = src.get("author")
        return f"{name} ({author})" if author else str(name)
    return str(src)


VERIFICATION_FILE = "verification.json"


def _load_verification(root: str) -> Dict[str, str]:
    """Maintainer-set reproduction status per entry, keyed "<tier>/<name>".

    Lives next to the submissions root (repo-root `verification.json`), outside
    `submissions/`, so the CI guard keeps submission PRs from editing it. Each
    value is a status string or an object with a `status` field."""
    path = os.path.join(os.path.dirname(os.path.normpath(root)), VERIFICATION_FILE)
    if not os.path.exists(path):
        return {}
    with open(path, encoding="utf-8-sig") as fh:
        data = json.load(fh)
    out = {}
    for key, v in data.items():
        if key.startswith("_"):
            continue
        status = v.get("status") if isinstance(v, dict) else v
        if status:
            out[key] = str(status)
    return out


def _render_leaderboard_body(root: str, heading: str = "##") -> List[str]:
    """The per-tier tables, one `<heading> <tier>` section each."""
    from .scorer import score_submission_set, rank_submissions, pareto_frontier
    tier_dirs = _tier_dir_map()
    order = ["intro", "hard", "scale", "stress", "congested", "designs"]
    lines: List[str] = []
    any_tier = False
    for tier in order:
        troot = os.path.join(root, tier)
        suite_dir = tier_dirs.get(tier)
        if not (os.path.isdir(troot) and suite_dir and
                os.path.exists(os.path.join(suite_dir, "suite.json"))):
            continue
        entries = list(_submission_entries(troot))
        if not entries:
            continue
        man = _load_manifest(suite_dir)
        subs = [score_submission_set(man, suite_dir, d, name, _load_runtimes(d))
                for name, d in entries]
        ranked = rank_submissions(subs)
        frontier = set(pareto_frontier(subs))
        metas = {name: _load_meta(d) for name, d in entries}
        verified = _load_verification(root)
        any_tier = True
        lines.append(f"{heading} {tier}  ({man['n_cases']} cases)")
        lines.append("")
        lines.append("| rank | submission | author | aggregate | legal | total delay | runtime (s) | Pareto | verified |")
        lines.append("|---:|---|---|---:|:---:|---:|---:|:---:|:---:|")
        derived = []
        for i, s in enumerate(ranked, 1):
            agg = f"{s.aggregate:.4f}" if s.complete else "—"
            td = s.total_delay if s.total_delay is not None else "—"
            rt = f"{s.total_runtime:.2f}" if s.total_runtime is not None else "—"
            author = _md_cell(metas.get(s.name, {}).get("author") or "—")
            upstream = _derived_from(metas.get(s.name, {}))
            mark = ""
            if upstream:
                mark = " †"
                derived.append(f"{_md_cell(s.name)} builds on {_md_cell(upstream)}")
            lines.append(f"| {i} | {_md_cell(s.name)}{mark} | {author} | {agg} | "
                         f"{s.n_legal}/{s.n_cases} | {td} | {rt} | "
                         f"{'✓' if s.name in frontier else ''} | "
                         f"{_md_cell(verified.get(f'{tier}/{s.name}', '—'))} |")
        lines.append("")
        if derived:
            lines.append("† derivative entry (refines another entry's routes): "
                         + "; ".join(derived) + ".")
            lines.append("")
    if not any_tier:
        lines.append("_No submissions yet. See CONTRIBUTING.md to add one._")
        lines.append("")
    return lines


_LEGEND = ["Ranked per tier (each tier is normalized to its own baseline, so a",
           "**higher aggregate is better** and the baseline itself scores 1.0000).",
           "`✓` marks submissions on the runtime-vs-total-delay Pareto frontier;",
           "`†` marks derivative entries that start from another entry's routes.",
           "`verified` is set by the maintainers once they have re-run an entry's",
           "router and reproduced its routes (`verification.json`); `—` means not yet."]


def _render_leaderboard_md(root: str) -> str:
    """Render LEADERBOARD.md from every `submissions/<tier>/<name>/` present.

    Scores are recomputed from the submitted route files by the independent
    checker, so the table cannot be gamed by editing numbers -- only by
    submitting better (still legal) routes."""
    lines = ["# Leaderboard", ""] + _LEGEND + [
             "This file is generated by `python -m m3d.cli leaderboard-all`;",
             "do not edit it by hand.", ""]
    return "\n".join(lines + _render_leaderboard_body(root))


README_START = "<!-- leaderboard:start (generated by `python -m m3d.cli leaderboard-all`; do not edit) -->"
README_END = "<!-- leaderboard:end -->"


def _render_readme_block(root: str) -> str:
    """The leaderboard section embedded in README.md, between the markers."""
    lines = [README_START, "", "## Leaderboard", ""] + _LEGEND + [
             "Submit by pull request; see [CONTRIBUTING.md](CONTRIBUTING.md). "
             "Also in [LEADERBOARD.md](LEADERBOARD.md).", ""]
    lines += _render_leaderboard_body(root, heading="###")
    return "\n".join(lines).rstrip() + "\n\n" + README_END


def _splice_readme(readme: str, block: str) -> str:
    """Replace the marked leaderboard block in README text."""
    i = readme.find(README_START)
    j = readme.find(README_END)
    if i < 0 or j < i:
        raise ValueError("README.md has no leaderboard markers")
    return readme[:i] + block + readme[j + len(README_END):]


def cmd_leaderboard_all(args: argparse.Namespace) -> int:
    root = args.submissions_root
    targets = [(args.out, _render_leaderboard_md(root) + "\n")]
    if args.readme:
        with open(args.readme, encoding="utf-8") as fh:
            readme = fh.read()
        targets.append((args.readme, _splice_readme(readme, _render_readme_block(root))))
    if args.check:
        stale = []
        for path, want in targets:
            try:
                with open(path, encoding="utf-8") as fh:
                    current = fh.read()
            except FileNotFoundError:
                current = ""
            if current.strip() != want.strip():
                stale.append(path)
        if stale:
            print(f"ERROR: {', '.join(stale)} out of date. Run "
                  f"`python -m m3d.cli leaderboard-all` and commit the result.")
            return 1
        print(f"{', '.join(p for p, _ in targets)} up to date.")
        return 0
    for path, want in targets:
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(want)
        print(f"wrote {path}")
    return 0


def cmd_pareto(args: argparse.Namespace) -> int:
    from .scorer import score_submission_set, pareto_frontier
    from .viz import pareto_plot
    man = _load_manifest(args.suite)
    subs = [score_submission_set(man, args.suite, d, name, _load_runtimes(d))
            for name, d in _submission_entries(args.submissions_root)]
    frontier = pareto_frontier(subs)
    out = args.out or "pareto.png"
    pareto_plot(subs, out, frontier,
                title=f"Runtime vs total delay — {man.get('tier', args.suite)}")
    print(f"wrote Pareto plot -> {out} (frontier: {frontier})")
    return 0


def cmd_animate(args: argparse.Namespace) -> int:
    from . import anim
    if args.mode == "suite":
        out = args.out or "docs/suite_sweep.gif"
        anim.suite_sweep_gif(args.suite, out, max_cases=args.max_cases)
        print(f"wrote animation -> {out}")
        return 0
    if not args.case:
        print("m3d animate: error: --mode layers requires --case", file=sys.stderr)
        return 2
    inst = Instance.load(args.case)
    if args.sol:
        sub = Submission.load(args.sol)
    else:
        sub, _ = route(inst)
        if sub is None:
            print("could not route case for animation", file=sys.stderr)
            return 2
    out = args.out or (os.path.splitext(args.case)[0] + ".gif")
    anim.layer_sweep_gif(inst, sub, out)
    print(f"wrote animation -> {out}")
    return 0


def cmd_info(args: argparse.Namespace) -> int:
    inst = Instance.load(args.case)
    n_cross = 0
    p2n = inst.pin_to_net()
    for net in inst.nets:
        dies = {inst.pin_by_id()[p].die for p in net.pins()}
        if len(dies) > 1:
            n_cross += 1
    fanouts = [len(n.sinks) for n in inst.nets]
    print(f"{inst.name}: {inst.width}x{inst.height}x{inst.layers} grid")
    print(f"  layer_delay={inst.layer_delay} via_delay={inst.via_delay}")
    print(f"  cells={len(inst.cells)} pins={len(inst.pins)} nets={len(inst.nets)}")
    print(f"  cross-die nets={n_cross} ({100*n_cross/max(1,len(inst.nets)):.0f}%)")
    print(f"  fanout: min={min(fanouts)} max={max(fanouts)} "
          f"avg={sum(fanouts)/len(fanouts):.2f}")
    print(f"  seed={inst.seed} master_seed={inst.master_seed}")
    return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="m3d", description="M3D routing challenge toolkit")
    sub = p.add_subparsers(dest="cmd", required=True)

    g = sub.add_parser("generate", help="build a deterministic benchmark tier")
    g.add_argument("--tier", default="intro", choices=["intro", "hard", "scale", "stress", "congested", "designs", "all"])
    g.add_argument("--out", default=None, help="output dir (defaults per tier)")
    g.add_argument("--layers", type=int, default=6)
    g.add_argument("--master-seed", type=int, default=20260923, dest="master_seed")
    g.set_defaults(func=cmd_generate)

    idn = sub.add_parser("import-design",
                         help="build a routing instance from a BLIF netlist")
    idn.add_argument("--blif", required=True, help="input .blif netlist")
    idn.add_argument("--name", required=True, help="instance name")
    idn.add_argument("--out", required=True, help="output instance JSON")
    idn.add_argument("--reference", default=None,
                     help="also write the certified reference solution here")
    idn.add_argument("--channel", type=int, default=5,
                     help="routing channel width (roominess); grows on failure")
    idn.add_argument("--layers", type=int, default=6)
    idn.add_argument("--router", default="negotiated",
                     choices=["baseline", "negotiated"])
    idn.set_defaults(func=cmd_import_design)

    b = sub.add_parser("baseline", help="run the baseline router on one case")
    b.add_argument("--case", required=True)
    b.add_argument("--out", default=None)
    b.add_argument("--order", default="bbox_desc",
                   choices=["bbox_desc", "bbox_asc", "id"])
    b.add_argument("--rip-limit", type=int, default=40, dest="rip_limit")
    b.add_argument("--ops-factor", type=int, default=200, dest="ops_factor")
    b.set_defaults(func=cmd_baseline)

    bs = sub.add_parser("baseline-suite",
                        help="run each case's baseline router (per suite.json)")
    bs.add_argument("--suite", default="benchmarks")
    bs.add_argument("--out-dir", required=True, dest="out_dir")
    bs.add_argument("--order", default="bbox_desc",
                    choices=["bbox_desc", "bbox_asc", "id"])
    bs.set_defaults(func=cmd_baseline_suite)

    e = sub.add_parser("evaluate", help="check + score one submission")
    e.add_argument("--case", required=True)
    e.add_argument("--sol", required=True)
    e.add_argument("--suite", default=None, help="suite dir for the baseline total")
    e.add_argument("--baseline", type=int, default=None, help="explicit baseline total")
    e.add_argument("--out", default=None)
    e.set_defaults(func=cmd_evaluate)

    s = sub.add_parser("score-suite", help="score a full submission for one tier")
    s.add_argument("--suite", default="benchmarks")
    s.add_argument("--submission-dir", required=True, dest="submission_dir")
    s.add_argument("--runtimes", default=None, help="optional JSON {case: seconds}")
    s.add_argument("--out", default=None)
    s.set_defaults(func=cmd_score_suite)

    v = sub.add_parser("visualize", help="render a case to PNG")
    v.add_argument("--case", required=True)
    v.add_argument("--sol", default=None)
    v.add_argument("--layer", type=int, default=None)
    v.add_argument("--out", default=None)
    v.add_argument("--show", action="store_true")
    v.set_defaults(func=cmd_visualize)

    rs = sub.add_parser("run-suite", help="route every case with a router, timing each")
    rs.add_argument("--suite", default="benchmarks")
    rs.add_argument("--router", default="baseline", choices=["baseline", "negotiated", "negotiated_fast", "negotiated2"])
    rs.add_argument("--out-dir", required=True, dest="out_dir")
    rs.set_defaults(func=cmd_run_suite)

    lb = sub.add_parser("leaderboard", help="rank multiple submissions for a suite")
    lb.add_argument("--suite", default="benchmarks")
    lb.add_argument("--submissions-root", required=True, dest="submissions_root",
                    help="dir with one subdir per submission (optional runtime.json each)")
    lb.add_argument("--out", default=None, help="leaderboard JSON")
    lb.add_argument("--md", default=None, help="leaderboard markdown")
    lb.set_defaults(func=cmd_leaderboard)

    la = sub.add_parser("leaderboard-all",
                        help="rebuild LEADERBOARD.md from submissions/<tier>/*")
    la.add_argument("--submissions-root", default="submissions", dest="submissions_root")
    la.add_argument("--out", default="LEADERBOARD.md")
    la.add_argument("--readme", default="README.md",
                    help="also refresh the leaderboard block in this file ('' to skip)")
    la.add_argument("--check", action="store_true",
                    help="fail if the on-disk file is stale (for CI)")
    la.set_defaults(func=cmd_leaderboard_all)

    pr = sub.add_parser("pareto", help="Pareto plot of runtime vs total delay")
    pr.add_argument("--suite", default="benchmarks")
    pr.add_argument("--submissions-root", required=True, dest="submissions_root")
    pr.add_argument("--out", default=None)
    pr.set_defaults(func=cmd_pareto)

    a = sub.add_parser("animate", help="render an animated GIF")
    a.add_argument("--mode", default="layers", choices=["layers", "suite"])
    a.add_argument("--case", default=None, help="case JSON (layers mode)")
    a.add_argument("--sol", default=None, help="solution (layers mode; baseline if omitted)")
    a.add_argument("--suite", default="benchmarks", help="suite dir (suite mode)")
    a.add_argument("--max-cases", type=int, default=None, dest="max_cases")
    a.add_argument("--out", default=None)
    a.set_defaults(func=cmd_animate)

    i = sub.add_parser("info", help="print a summary of a case")
    i.add_argument("--case", required=True)
    i.set_defaults(func=cmd_info)
    return p


def main(argv=None) -> int:
    args = build_parser().parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
