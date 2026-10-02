#!/usr/bin/env python3
"""Example participant router.

This is a deliberately small, self-contained router that shows how to:

* load an instance,
* build per-net routing trees on the shared 3D grid using the provided
  primitives (``m3d.grid.Grid``),
* keep to the capacity rules (one net per vertex/edge; never cross another net's
  pin),
* self-check with the independent checker before submitting,
* write a submission JSON in the required format.

Strategy: greedy sequential routing. Nets are routed one at a time (smallest
bounding box first); each net is grown as a tree with multi-source Dijkstra from
the driver, honoring hard capacity against already-routed nets. There is NO
rip-up here — that is what keeps this example short and distinct from the
baseline. If this simple strategy cannot route a net (a conflict it cannot avoid),
the script falls back to the provided routers for the whole instance -- the
simple baseline, then the negotiated-congestion router (which the contended tiers
need) -- so the submission is always complete and legal.

Replace ``greedy_route`` with your own algorithm to compete. Everything you need
is public: ``m3d.model`` (data + JSON), ``m3d.grid`` (graph moves),
``m3d.checker`` (validate locally), ``m3d.baseline`` and ``m3d.negotiated``
(reference routers).

Usage:
    python examples/example_submission.py --case benchmarks/case_01.json --out out.sol.json
    python examples/example_submission.py --suite benchmarks --out-dir examples/submissions
"""
from __future__ import annotations

import argparse
import heapq
import os
import sys
from typing import Dict, List, Optional, Set, Tuple

# make the repo root importable when this file is run directly (python examples/...)
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from m3d import baseline, negotiated
from m3d.checker import check
from m3d.grid import Grid, edge_key
from m3d.model import Instance, NetRoute, Submission


def greedy_route(inst: Instance) -> Optional[Submission]:
    """Return a legal Submission, or None if the greedy pass hit a conflict."""
    g = Grid(inst)
    pin_vid = {p.id: g.vid(p.vertex()) for p in inst.pins}
    pin_owner = {pin_vid[p]: n.id for n in inst.nets for p in n.pins()}

    vertex_owner: Dict[int, int] = {}
    edge_owner: Dict[Tuple[int, int], int] = {}

    # smallest bounding box first (a different order from the baseline)
    def bbox(net) -> int:
        cs = [g.coord(pin_vid[p]) for p in net.pins()]
        xs = [c[0] for c in cs]; ys = [c[1] for c in cs]; zs = [c[2] for c in cs]
        return (max(xs) - min(xs)) + (max(ys) - min(ys)) + (max(zs) - min(zs))

    routes: List[NetRoute] = []
    for net in sorted(inst.nets, key=bbox):
        pins = [pin_vid[p] for p in net.pins()]
        forbidden_pins = {v for v, nid in pin_owner.items() if nid != net.id}
        tree_v: Set[int] = {pins[0]}
        tree_e: Set[Tuple[int, int]] = set()
        remaining = set(pins[1:])
        while remaining:
            dist: Dict[int, int] = {}
            prev: Dict[int, int] = {}
            heap: List[Tuple[int, int]] = [(0, s) for s in tree_v]
            for s in tree_v:
                dist[s] = 0
            heapq.heapify(heap)
            found = None
            while heap:
                d, u = heapq.heappop(heap)
                if d > dist.get(u, d):
                    continue
                if u in remaining:
                    found = u
                    break
                for v, w in g.neighbors(u):
                    if v in forbidden_pins or v in tree_v:
                        continue
                    if v in vertex_owner and vertex_owner[v] != net.id:
                        continue
                    ek = edge_key(u, v)
                    if ek in edge_owner and edge_owner[ek] != net.id:
                        continue
                    nd = d + w
                    if nd < dist.get(v, 1 << 62):
                        dist[v] = nd
                        prev[v] = u
                        heapq.heappush(heap, (nd, v))
            if found is None:
                return None  # greedy failed; caller falls back to the reference routers
            path = [found]
            cur = found
            while cur in prev:
                cur = prev[cur]
                path.append(cur)
            for i in range(len(path) - 1):
                a, b = path[i], path[i + 1]
                tree_v.add(a); tree_v.add(b)
                tree_e.add(edge_key(a, b))
            remaining.discard(found)
        for v in tree_v:
            vertex_owner[v] = net.id
        for e in tree_e:
            edge_owner[e] = net.id
        routes.append(NetRoute(net=net.id,
                               edges=[(g.coord(a), g.coord(b)) for (a, b) in tree_e]))
    return Submission(instance=inst.name, routes=routes)


def route_instance(inst: Instance) -> Submission:
    sub = greedy_route(inst)
    if sub is not None and check(inst, sub).legal:
        return sub
    # fall back to the provided routers so the submission is always complete:
    # the simple baseline, then the negotiated-congestion router (the reference
    # for the contended tiers, where the simple baseline fails)
    sub, _ = baseline.route(inst)
    if sub is None:
        sub, _ = negotiated.route_negotiated(inst)
    if sub is None:
        raise RuntimeError(f"could not route {inst.name} "
                           f"(greedy, baseline and negotiated all failed)")
    return sub


def main() -> int:
    ap = argparse.ArgumentParser(description="example participant router")
    ap.add_argument("--case")
    ap.add_argument("--out")
    ap.add_argument("--suite")
    ap.add_argument("--out-dir", dest="out_dir")
    args = ap.parse_args()

    if args.suite:
        import json
        man = json.load(open(os.path.join(args.suite, "suite.json")))
        os.makedirs(args.out_dir, exist_ok=True)
        import time as _time
        runtimes = {}
        for c in man["cases"]:
            inst = Instance.load(os.path.join(args.suite, c["instance_file"]))
            t0 = _time.time()
            sub = route_instance(inst)
            runtimes[inst.name] = round(_time.time() - t0, 3)
            res = check(inst, sub)
            out = os.path.join(args.out_dir, f"{inst.name}.sol.json")
            sub.save(out)
            print(f"{inst.name}: legal={res.legal} total={res.total_delay} -> {out}")
        json.dump(runtimes, open(os.path.join(args.out_dir, "runtime.json"), "w"), indent=1)
        return 0

    inst = Instance.load(args.case)
    sub = route_instance(inst)
    res = check(inst, sub)
    out = args.out or (os.path.splitext(args.case)[0] + ".example.sol.json")
    sub.save(out)
    print(f"{inst.name}: legal={res.legal} total={res.total_delay} -> {out}")
    return 0 if res.legal else 1


if __name__ == "__main__":
    raise SystemExit(main())
