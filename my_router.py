#!/usr/bin/env python3
"""My M3D router (pure standard library).

Pipeline per case
-----------------
1. Lower bound: route every net alone (only pins block) with an exact
   shortest-path tree.  The sum is a true lower bound on total delay, so at the
   end we can print how close to optimal we are ("gap").
2. Construct a legal solution:
     a. greedy, hard-capacity, exact shortest-path-tree (SPT) routing in a few
        net orders, keep the best that fully routes;
     b. otherwise PathFinder-style negotiated congestion (with SPT trees);
     c. otherwise fall back to the provided negotiated / baseline routers.
3. Polish: rip up one net at a time and reroute it as the exact SPT against all
   other nets fixed; accept only strict improvements; repeat to a fixpoint.
4. LNS: rip up a few nearby nets, reroute them in random orders, accept if the
   total delay does not get worse.  Runs until the time budget is used up or
   the lower bound is reached.

Why SPT?  The objective is the SUM OVER SINKS of the driver->sink path delay,
so the optimal tree for one net (others fixed) is the shortest-path tree from
the driver.  The provided baseline grows "nearest sink to the tree" (a Prim
style tree), which is cheaper in wire but worse for this objective.

Legality: every route vertex is owned by exactly one net (edges follow from
that), a net never enters another net's pin, and trees come from Dijkstra
parent pointers so they are acyclic.  The final answer is always re-verified
with m3d.checker and replaced by a provided router's answer if it fails.

Usage
-----
    python my_router.py --case benchmarks/case_01.json --out my.sol.json
    python my_router.py --suite benchmarks --out-dir submissions/intro/mine \
                        --time 15 --author yourname
"""
from __future__ import annotations

import argparse
import heapq
import json
import os
import random
import sys
import time
from typing import Dict, List, Optional, Set, Tuple

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from m3d import baseline
from m3d.checker import check
from m3d.model import Instance, NetRoute, Submission

K = 1 << 22          # delay*K + hops : exact delay first, fewest vertices second
INF = float("inf")


class Router:
    def __init__(self, inst: Instance, seed: int = 1):
        self.inst = inst
        self.W, self.H, self.L = inst.width, inst.height, inst.layers
        self.wh = self.W * self.H
        self.N = self.wh * self.L
        self.step_layer = [d * K + 1 for d in inst.layer_delay]
        self.step_via = inst.via_delay * K + 1
        self.base = float(min(inst.layer_delay))
        self.rng = random.Random(seed)
        # huge grids: restrict each search to the net bbox + margin first
        self.margin: Optional[int] = 12
        self.lb_margin: Optional[int] = 24

        pv = inst.pin_vertex()
        self.nids = [n.id for n in inst.nets]
        self.drv: Dict[int, int] = {}
        self.sinks: Dict[int, List[int]] = {}
        self.pbb: Dict[int, Tuple[int, int, int, int]] = {}
        self.pin_of = [-1] * self.N
        for n in inst.nets:
            d = self.vid(pv[n.driver])
            ss = [self.vid(pv[s]) for s in n.sinks]
            self.drv[n.id] = d
            self.sinks[n.id] = ss
            xs, ys = [], []
            for v in [d] + ss:
                x, y, _ = self.coord(v)
                xs.append(x); ys.append(y)
                self.pin_of[v] = n.id
            self.pbb[n.id] = (min(xs), min(ys), max(xs), max(ys))

        self.dist = [INF] * self.N
        self.prv = [0] * self.N
        self.cnt = [0] * self.N
        self.hist: Dict[int, float] = {}
        self.over: Set[int] = set()
        self.par: Dict[int, Dict[int, int]] = {}
        self.nodes: Dict[int, Set[int]] = {}
        self.delay: Dict[int, int] = {}
        self.bb: Dict[int, Tuple[int, int, int, int]] = {}
        self.lb: Dict[int, int] = {}

    # ---- coordinates ------------------------------------------------------
    def vid(self, v) -> int:
        x, y, z = v
        return (z * self.H + y) * self.W + x

    def coord(self, vid: int):
        z, r = divmod(vid, self.wh)
        y, x = divmod(r, self.W)
        return (x, y, z)

    def _ew(self, a: int, b: int) -> int:
        if abs(a - b) == self.wh:
            return self.inst.via_delay
        return self.inst.layer_delay[a // self.wh]

    # ---- state ------------------------------------------------------------
    def reset(self) -> None:
        for nid in list(self.par):
            self._remove(nid)
        self.hist.clear()

    def _tree_delay(self, nid: int, parent: Dict[int, int]) -> int:
        drv = self.drv[nid]
        dd = {drv: 0}
        for s in self.sinks[nid]:
            path = []
            v = s
            while v not in dd:
                path.append(v)
                v = parent[v]
            base = dd[v]
            for u in reversed(path):
                base += self._ew(parent[u], u)
                dd[u] = base
        return sum(dd[s] for s in self.sinks[nid])

    def _commit(self, nid: int, parent: Dict[int, int], delay: Optional[int] = None) -> None:
        nodes = set(parent)
        nodes.add(self.drv[nid])
        cnt, over = self.cnt, self.over
        for v in nodes:
            cnt[v] += 1
            if cnt[v] > 1:
                over.add(v)
        self.par[nid] = parent
        self.nodes[nid] = nodes
        self.delay[nid] = self._tree_delay(nid, parent) if delay is None else delay
        xs = []; ys = []
        for v in nodes:
            r = v % self.wh
            ys.append(r // self.W); xs.append(r % self.W)
        self.bb[nid] = (min(xs), min(ys), max(xs), max(ys))

    def _remove(self, nid: int) -> None:
        cnt, over = self.cnt, self.over
        for v in self.nodes.pop(nid):
            cnt[v] -= 1
            if cnt[v] < 2:
                over.discard(v)
        del self.par[nid]
        del self.delay[nid]
        del self.bb[nid]

    def total(self) -> int:
        return sum(self.delay.values())

    def snapshot(self):
        return {n: (dict(self.par[n]), self.delay[n]) for n in self.par}

    def restore(self, snap) -> None:
        self.reset()
        for n, (p, d) in snap.items():
            self._commit(n, p, d)

    # ---- exact SPT with hard capacity ------------------------------------
    def _box(self, nid: int, margin: Optional[int]):
        if margin is None:
            return 0, self.W - 1, 0, self.H - 1
        x0, y0, x1, y1 = self.pbb[nid]
        return (max(0, x0 - margin), min(self.W - 1, x1 + margin),
                max(0, y0 - margin), min(self.H - 1, y1 + margin))

    def spt_hard(self, nid: int):
        r = self._spt_hard_box(nid, self.margin)
        if r is None and self.margin is not None:
            r = self._spt_hard_box(nid, None)
        return r

    def _spt_hard_box(self, nid: int, margin: Optional[int]):
        W, wh, Lm1 = self.W, self.wh, self.L - 1
        cnt, pin_of, dist, prv = self.cnt, self.pin_of, self.dist, self.prv
        sl, sv = self.step_layer, self.step_via
        xmin, xmax, ymin, ymax = self._box(nid, margin)
        drv = self.drv[nid]
        sinks = self.sinks[nid]
        need = set(sinks)
        remaining = len(need)
        dist[drv] = 0
        touched = [drv]
        heap = [(0, drv)]
        pop, push = heapq.heappop, heapq.heappush
        while heap:
            d, u = pop(heap)
            if d > dist[u]:
                continue
            if u in need:
                need.discard(u)
                remaining -= 1
                if not remaining:
                    break
            z, r = divmod(u, wh)
            y, x = divmod(r, W)
            c = sl[z]
            nb = []
            if x < xmax: nb.append((u + 1, c))
            if x > xmin: nb.append((u - 1, c))
            if y < ymax: nb.append((u + W, c))
            if y > ymin: nb.append((u - W, c))
            if z < Lm1: nb.append((u + wh, sv))
            if z > 0: nb.append((u - wh, sv))
            for v, c2 in nb:
                if cnt[v]:
                    continue
                p = pin_of[v]
                if p != -1 and p != nid:
                    continue
                nd = d + c2
                if nd < dist[v]:
                    if dist[v] == INF:
                        touched.append(v)
                    dist[v] = nd
                    prv[v] = u
                    push(heap, (nd, v))
        res = None
        if not remaining:
            delay = sum(dist[s] // K for s in sinks)
            parent: Dict[int, int] = {}
            for s in sinks:
                v = s
                while v != drv and v not in parent:
                    parent[v] = prv[v]
                    v = prv[v]
            res = (parent, delay)
        for v in touched:
            dist[v] = INF
        return res

    # ---- SPT with soft congestion costs (negotiation) --------------------
    def spt_cong(self, nid: int, pf: float):
        r = self._spt_cong_box(nid, pf, self.margin)
        if r is None and self.margin is not None:
            r = self._spt_cong_box(nid, pf, None)
        return r

    def _spt_cong_box(self, nid: int, pf: float, margin: Optional[int]):
        W, wh, Lm1 = self.W, self.wh, self.L - 1
        cnt, pin_of, dist, prv, hist = self.cnt, self.pin_of, self.dist, self.prv, self.hist
        ld, via, base = self.inst.layer_delay, self.inst.via_delay, self.base
        xmin, xmax, ymin, ymax = self._box(nid, margin)
        drv = self.drv[nid]
        sinks = self.sinks[nid]
        need = set(sinks)
        remaining = len(need)
        dist[drv] = 0.0
        touched = [drv]
        heap = [(0.0, drv)]
        pop, push = heapq.heappop, heapq.heappush
        while heap:
            d, u = pop(heap)
            if d > dist[u]:
                continue
            if u in need:
                need.discard(u)
                remaining -= 1
                if not remaining:
                    break
            z, r = divmod(u, wh)
            y, x = divmod(r, W)
            c = ld[z]
            nb = []
            if x < xmax: nb.append((u + 1, c))
            if x > xmin: nb.append((u - 1, c))
            if y < ymax: nb.append((u + W, c))
            if y > ymin: nb.append((u - W, c))
            if z < Lm1: nb.append((u + wh, via))
            if z > 0: nb.append((u - wh, via))
            for v, c2 in nb:
                p = pin_of[v]
                if p != -1 and p != nid:
                    continue
                nd = d + c2 + 1e-6 + base * (hist.get(v, 0.0) + pf * cnt[v])
                if nd < dist[v]:
                    if dist[v] == INF:
                        touched.append(v)
                    dist[v] = nd
                    prv[v] = u
                    push(heap, (nd, v))
        res = None
        if not remaining:
            parent: Dict[int, int] = {}
            for s in sinks:
                v = s
                while v != drv and v not in parent:
                    parent[v] = prv[v]
                    v = prv[v]
            res = parent
        for v in touched:
            dist[v] = INF
        return res

    # ---- orders -----------------------------------------------------------
    def _bbox_len(self, nid: int) -> int:
        x0, y0, x1, y1 = self.pbb[nid]
        return (x1 - x0) + (y1 - y0)

    def order(self, kind: str) -> List[int]:
        ids = list(self.nids)
        if kind == "asc":
            ids.sort(key=lambda i: (self._bbox_len(i), i))
        elif kind == "desc":
            ids.sort(key=lambda i: (-self._bbox_len(i), i))
        elif kind == "fanout":
            ids.sort(key=lambda i: (-len(self.sinks[i]), -self._bbox_len(i), i))
        return ids

    # ---- lower bound ------------------------------------------------------
    def compute_lb(self) -> int:
        self.reset()
        for nid in self.nids:
            r = self._spt_hard_box(nid, self.lb_margin)
            if r is None:
                r = self._spt_hard_box(nid, None)
            self.lb[nid] = r[1] if r else 0
        return sum(self.lb.values())

    # ---- construction -----------------------------------------------------
    def greedy(self, order: List[int]) -> bool:
        self.reset()
        for nid in order:
            r = self.spt_hard(nid)
            if r is None:
                return False
            self._commit(nid, r[0], r[1])
        return True

    def negotiate(self, deadline: float, max_iters: int = 80, pf0: float = 0.5,
                  mult: float = 1.6, hist_fac: float = 0.6,
                  kind: str = "desc") -> bool:
        self.reset()
        order = self.order(kind)
        pf = pf0
        for nid in order:
            p = self.spt_cong(nid, pf)
            if p is None:
                return False
            self._commit(nid, p)
        it = 0
        while self.over and it < max_iters and time.time() < deadline:
            it += 1
            for v in self.over:
                self.hist[v] = self.hist.get(v, 0.0) + hist_fac * (self.cnt[v] - 1)
            pf = min(pf * mult, 1e6)
            over = self.over
            affected = [n for n in order if not self.nodes[n].isdisjoint(over)]
            for nid in affected:
                self._remove(nid)
                p = self.spt_cong(nid, pf)
                if p is None:
                    return False
                self._commit(nid, p)
        return not self.over

    def load_submission(self, sub: Submission) -> bool:
        self.reset()
        byn = {r.net: r for r in sub.routes}
        for nid in self.nids:
            adj: Dict[int, List[int]] = {}
            for a, b in byn[nid].edges:
                va, vb = self.vid(a), self.vid(b)
                adj.setdefault(va, []).append(vb)
                adj.setdefault(vb, []).append(va)
            drv = self.drv[nid]
            parent: Dict[int, int] = {}
            seen = {drv}
            stack = [drv]
            while stack:
                u = stack.pop()
                for v in adj.get(u, ()):
                    if v not in seen:
                        seen.add(v)
                        parent[v] = u
                        stack.append(v)
            if any(s not in seen for s in self.sinks[nid]):
                return False
            self._commit(nid, parent)
        return not self.over

    # ---- improvement ------------------------------------------------------
    def polish(self, deadline: float) -> None:
        while time.time() < deadline:
            cand = [n for n in self.nids if self.delay[n] > self.lb.get(n, 0)]
            if not cand:
                return
            cand.sort(key=lambda n: -(self.delay[n] - self.lb.get(n, 0)))
            improved = False
            for nid in cand:
                if time.time() >= deadline:
                    return
                old_p, old_d = self.par[nid], self.delay[nid]
                self._remove(nid)
                r = self.spt_hard(nid)
                if r is not None and r[1] < old_d:
                    self._commit(nid, r[0], r[1])
                    improved = True
                else:
                    self._commit(nid, old_p, old_d)
            if not improved:
                return

    def lns(self, deadline: float, lb_total: int, max_stall: int = 600) -> None:
        rng = self.rng
        stall = 0
        while time.time() < deadline and self.total() > lb_total and stall < max_stall:
            gapnets = [n for n in self.nids if self.delay[n] > self.lb.get(n, 0)]
            if not gapnets:
                return
            seed_net = rng.choice(gapnets)
            sx0, sy0, sx1, sy1 = self.bb[seed_net]
            m = 2 + stall // 40
            nearby = []
            for n in self.nids:
                if n == seed_net:
                    continue
                x0, y0, x1, y1 = self.bb[n]
                if x0 <= sx1 + m and x1 >= sx0 - m and y0 <= sy1 + m and y1 >= sy0 - m:
                    nearby.append(n)
            k = rng.randint(1, min(len(nearby), 3 + stall // 60)) if nearby else 0
            group = [seed_net] + rng.sample(nearby, k)
            old = {n: (self.par[n], self.delay[n]) for n in group}
            old_total = sum(d for _, d in old.values())
            for n in group:
                self._remove(n)
            rng.shuffle(group)
            done: List[int] = []
            new_total = 0
            ok = True
            for n in group:
                r = self.spt_hard(n)
                if r is None:
                    ok = False
                    break
                self._commit(n, r[0], r[1])
                done.append(n)
                new_total += r[1]
            if ok and new_total <= old_total:
                stall = 0 if new_total < old_total else stall + 1
            else:
                for n in done:
                    self._remove(n)
                for n, (p, d) in old.items():
                    self._commit(n, p, d)
                stall += 1

    # ---- export -----------------------------------------------------------
    def to_submission(self) -> Submission:
        routes = []
        for nid in self.nids:
            edges = [(self.coord(v), self.coord(p)) for v, p in self.par[nid].items()]
            routes.append(NetRoute(net=nid, edges=edges))
        return Submission(instance=self.inst.name, routes=routes)


def _fallback(inst: Instance) -> Optional[Submission]:
    sub, _ = baseline.route(inst)
    if sub is not None:
        return sub
    from m3d.negotiated import route_negotiated
    sub, _ = route_negotiated(inst)
    return sub


def route_instance(inst: Instance, time_limit: float = 15.0,
                   verbose: bool = True) -> Tuple[Submission, Dict]:
    t0 = time.time()
    deadline = t0 + time_limit
    R = Router(inst)
    lb_total = R.compute_lb()
    info = {"lb": lb_total, "method": None}

    # 1) greedy SPT in several orders (fast)
    best = None            # (total, snapshot, method)
    for kind in ("asc", "desc", "fanout"):
        if R.greedy(R.order(kind)):
            tot = R.total()
            if best is None or tot < best[0]:
                best = (tot, R.snapshot(), "greedy")
            if tot == lb_total:
                break
    # 2) negotiated congestion: slower, but globally better when nets interact
    if best is None or best[0] > lb_total:
        neg_deadline = t0 + max(0.75 * time_limit, 1.0) if best is not None else deadline + 120
        for kind, args in (("desc", {}),
                           ("desc", {"pf0": 0.3, "mult": 1.3, "hist_fac": 0.4, "max_iters": 150}),
                           ("asc", {})):
            if best is not None and time.time() >= neg_deadline:
                break
            if R.negotiate(neg_deadline, kind=kind, **args):
                tot = R.total()
                if best is None or tot < best[0]:
                    best = (tot, R.snapshot(), "negotiated")
                break
    if best is not None:
        R.restore(best[1])
        info["method"] = best[2]
    else:
        # 3) provided routers
        sub = _fallback(inst)
        if sub is None:
            raise RuntimeError(f"{inst.name}: could not find any legal routing")
        R.load_submission(sub)
        info["method"] = "fallback"

    start_total = R.total()
    if R.total() > lb_total:
        R.polish(deadline)
        R.lns(deadline, lb_total)

    sub = R.to_submission()
    res = check(inst, sub)
    if not res.legal:
        sub = _fallback(inst)
        res = check(inst, sub)
        info["method"] = "fallback(after-illegal)"
    info.update(total=res.total_delay, start=start_total, legal=res.legal,
                secs=round(time.time() - t0, 2))
    return sub, info


def main() -> int:
    ap = argparse.ArgumentParser(description="my M3D router")
    ap.add_argument("--case")
    ap.add_argument("--out")
    ap.add_argument("--suite")
    ap.add_argument("--out-dir", dest="out_dir")
    ap.add_argument("--time", type=float, default=15.0,
                    help="seconds of polish/LNS per case (default 15)")
    ap.add_argument("--author", default="")
    ap.add_argument("--only", default=None, help="comma list of case names")
    args = ap.parse_args()

    if args.suite:
        with open(os.path.join(args.suite, "suite.json"), encoding="utf-8") as fh:
            man = json.load(fh)
        os.makedirs(args.out_dir, exist_ok=True)
        only = set(args.only.split(",")) if args.only else None
        runtimes = {}
        for c in man["cases"]:
            if only and c["name"] not in only:
                continue
            inst = Instance.load(os.path.join(args.suite, c["instance_file"]))
            sub, info = route_instance(inst, args.time)
            sub.save(os.path.join(args.out_dir, f"{inst.name}.sol.json"))
            runtimes[inst.name] = info["secs"]
            ratio = c["baseline_total"] / info["total"] if info["total"] else 0.0
            gap = 100.0 * (info["total"] - info["lb"]) / info["lb"] if info["lb"] else 0.0
            print(f"{inst.name}: legal={info['legal']} total={info['total']} "
                  f"baseline={c['baseline_total']} ratio={ratio:.4f} "
                  f"lb={info['lb']} gap={gap:.2f}% start={info['start']} "
                  f"via={info['method']} {info['secs']}s", flush=True)
        with open(os.path.join(args.out_dir, "runtime.json"), "w", encoding="utf-8") as fh:
            json.dump(runtimes, fh, indent=1)
        with open(os.path.join(args.out_dir, "meta.json"), "w", encoding="utf-8") as fh:
            json.dump({"author": args.author,
                       "description": "exact SPT + negotiation + polish + LNS"}, fh, indent=1)
        return 0

    inst = Instance.load(args.case)
    sub, info = route_instance(inst, args.time)
    out = args.out or (os.path.splitext(args.case)[0] + ".mine.sol.json")
    sub.save(out)
    print(f"{inst.name}: {info} -> {out}")
    return 0 if info["legal"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
