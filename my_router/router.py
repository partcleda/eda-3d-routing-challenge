
#!/usr/bin/env python3
"""Shortest-path-tree router + rip-up-and-reroute refinement passes."""
from __future__ import annotations
import argparse, heapq, json, os, sys, time
from typing import Dict, List, Optional, Set, Tuple
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
from m3d import baseline, negotiated
from m3d.checker import check
from m3d.grid import Grid, edge_key
from m3d.model import Instance, NetRoute, Submission
 
INF = 1 << 62
LNS_SECONDS = 20
LNS_SECONDS_PER_NET = 0.4
 
# exp 7: introduce ideal path conflict optimization. Replaced sinks ordering with conflicts ordering, so the constants were not kept entirely.
class Router:
    def __init__(self, inst):
        self.inst = inst
        self.g = Grid(inst)
        g = self.g
        self.pin_vid = {p.id: g.vid(p.vertex()) for p in inst.pins}
        self.net_pins = {n.id: [self.pin_vid[p] for p in n.pins()] for n in inst.nets}
        self.pin_owner = {v: nid for nid, ps in self.net_pins.items() for v in ps}
        self.vertex_owner: Dict[int, int] = {}
        self.edge_owner: Dict[Tuple[int, int], int] = {}
        self.trees: Dict[int, Tuple[Set[int], Set[Tuple[int, int]], int]] = {}
 
    def bbox(self, nid):
        cs = [self.g.coord(v) for v in self.net_pins[nid]]
        return sum(max(c[i] for c in cs) - min(c[i] for c in cs) for i in range(3))
 
    def route_net(self, nid, ignore_wires=False):
        """Shortest-path tree from the driver. Returns (tree_v, tree_e, cost) or None."""
        g = self.g
        pins = self.net_pins[nid]
        driver, sinks = pins[0], set(pins[1:])
        dist = {driver: 0}; prev = {}
        heap = [(0, driver)]; unreached = set(sinks)
        while heap and unreached:
            d, u = heapq.heappop(heap)
            if d > dist[u]:
                continue
            unreached.discard(u)
            for v, w in g.neighbors(u):
                po = self.pin_owner.get(v)
                if po is not None and po != nid:
                    continue
                if not ignore_wires:
                    vo = self.vertex_owner.get(v)
                    if vo is not None and vo != nid:
                        continue
                    eo = self.edge_owner.get(edge_key(u, v))
                    if eo is not None and eo != nid:
                        continue
                nd = d + w
                if nd < dist.get(v, INF):
                    dist[v] = nd; prev[v] = u
                    heapq.heappush(heap, (nd, v))
        if unreached:
            return None
        tv = {driver}; te = set()
        for s in sinks:
            cur = s
            while cur != driver and cur not in tv:
                tv.add(cur); te.add(edge_key(cur, prev[cur])); cur = prev[cur]
        return tv, te, sum(dist[s] for s in sinks)
 
    def commit(self, nid, tree):
        tv, te, _ = tree
        for v in tv: self.vertex_owner[v] = nid
        for e in te: self.edge_owner[e] = nid
        self.trees[nid] = tree
 
    def uncommit(self, nid):
        tv, te, _ = self.trees.pop(nid)
        for v in tv: del self.vertex_owner[v]
        for e in te: del self.edge_owner[e]
 
    def initial(self, order):
        for nid in order:
            t = self.route_net(nid)
            if t is None:
                return False
            self.commit(nid, t)
        return True
 
    def refine(self, passes):
        """Rip up each net and reroute it given everyone else. The old route is
        still available, so the new cost is never worse."""
        for _ in range(passes):
            improved = 0
            for nid in sorted(self.trees, key=lambda n: -self.trees[n][2]):
                old = self.trees[nid]
                self.uncommit(nid)
                t = self.route_net(nid)
                if t is None or t[2] >= old[2]:
                    self.commit(nid, old)
                else:
                    self.commit(nid, t); improved += old[2] - t[2]
            if improved == 0:
                break
 
    def lns(self, deadline):
        """For nets that are worse than their unobstructed ideal, rip up the net
        plus the nets in its way, route the target first, re-route the rest, and
        keep the change only if the total drops."""
        # improvement: compute ideal moves only once
        ideals = {}
        for nid in self.trees:
            ideal = self.route_net(nid, ignore_wires=True)
            if ideal:
                ideals[nid] = ideal

        while time.time() < deadline:
            gains = []
            for nid, tr in self.trees.items():
                ideal = ideals.get(nid)
                if ideal and ideal[2] < tr[2]:
                    gains.append((tr[2] - ideal[2], nid, ideal))
            gains.sort(reverse=True)
            changed = False
            for gain, nid, ideal in gains:
                if time.time() > deadline:
                    break
                blockers = {self.vertex_owner[v] for v in ideal[0]
                            if self.vertex_owner.get(v, nid) != nid}
                blockers |= {self.edge_owner[e] for e in ideal[1]
                             if self.edge_owner.get(e, nid) != nid}
                group = [nid] + sorted(blockers, key=lambda n: -self.trees[n][2])
                saved = {n: self.trees[n] for n in group}
                before = sum(t[2] for t in saved.values())
                for n in group:
                    self.uncommit(n)
                ok = True; after = 0
                for n in group:
                    t = self.route_net(n)
                    if t is None:
                        ok = False; break
                    self.commit(n, t); after += t[2]
                if ok and after < before:
                    changed = True
                else:
                    for n in group:
                        if n in self.trees:
                            self.uncommit(n)
                    for n in group:
                        self.commit(n, saved[n])
            if not changed:
                break
 
    def total(self):
        return sum(t[2] for t in self.trees.values())
 
    def submission(self):
        g = self.g
        routes = [NetRoute(net=nid, edges=[(g.coord(a), g.coord(b)) for (a, b) in te])
                  for nid, (tv, te, _) in self.trees.items()]
        return Submission(instance=self.inst.name, routes=routes)
 
 
def route_instance(inst, passes=10):
    nets = [n.id for n in inst.nets]
    probe = Router(inst)
    # improvement: a new order to go by delay to see if this improves anything
    ideal_cost = {}
    ideal_tree = {}
    for n in nets:
        t = probe.route_net(n, ignore_wires=True)
        ideal_cost[n] = t[2] if t else 0
        ideal_tree[n] = t[0] if t else set()

    # how many nets' ideal routes want each grid point
    demand = {}
    for n in nets:
        for v in ideal_tree[n]:
            demand[v] = demand.get(v, 0) + 1

    # a net's conflict score: total overlap with other nets' ideal routes
    conflicts = {n: sum(demand[v] - 1 for v in ideal_tree[n]) for n in nets}

    orders = {
        "bbox_desc": sorted(nets, key=probe.bbox, reverse=True),
        #"sinks_desc": sorted(nets, key=lambda n: (-len(probe.net_pins[n]), -probe.bbox(n))),
        "delay_desc": sorted(nets, key=lambda n: ideal_cost[n], reverse=True),
        "conflict_desc": sorted(nets, key=lambda n: conflicts[n], reverse=True),
    }

    best = None
    for name, order in orders.items():
        r = Router(inst)
        if not r.initial(order):
            continue
        r.refine(passes)
        # Experiment 6: expand/contract by case size
        r.lns(time.time() + LNS_SECONDS_PER_NET * len(inst.nets))
        r.refine(passes)
        if best is None or r.total() < best.total():
            best = r
    if best is not None:
        sub = best.submission()
        if check(inst, sub).legal:
            return sub
    print(f"{inst.name}: MY ROUTER FAILED, falling back to baseline")
    sub, _ = baseline.route(inst)
    if sub is None:
        sub, _ = negotiated.route_negotiated(inst)
    return sub
 
 
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--suite"); ap.add_argument("--out-dir", dest="out_dir")
    ap.add_argument("--passes", type=int, default=10)
    a = ap.parse_args()
    man = json.load(open(os.path.join(a.suite, "suite.json")))
    os.makedirs(a.out_dir, exist_ok=True); rt = {}
    for c in man["cases"]:
        inst = Instance.load(os.path.join(a.suite, c["instance_file"]))
        t0 = time.time(); sub = route_instance(inst, a.passes); rt[inst.name] = round(time.time() - t0, 3)
        res = check(inst, sub); sub.save(os.path.join(a.out_dir, f"{inst.name}.sol.json"))
        print(f"{inst.name}: legal={res.legal} total={res.total_delay} t={rt[inst.name]}s")
    json.dump(rt, open(os.path.join(a.out_dir, "runtime.json"), "w"), indent=1)
 
if __name__ == "__main__":
    main()
 