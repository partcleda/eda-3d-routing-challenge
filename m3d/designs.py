"""Build routing-challenge instances from real open-source gate-level netlists.

This turns a **real design** (a BLIF gate-level netlist) into an M3D routing
instance that uses the exact same model, JSON format and tooling as the generated
tiers -- so the checker, scorer, baseline, negotiated router and visualizer all
work on it unchanged.

Mapping (netlist -> routing instance)
-------------------------------------
* Every logic node (``.names``), primary input, primary output and latch becomes
  a **cell**. A cell's footprint is the smallest square that holds its pins.
* Every **signal** that has a driver and at least one sink becomes a **net**: the
  driving terminal is the net's driver pin, the reading terminals are its sinks.
  (Dead outputs -- a driver with no readers -- create no pin, so every pin
  belongs to exactly one net, as the model requires.)
* Cells are ordered by **reverse Cuthill-McKee** (a connectivity-clustering order
  that keeps cells sharing a signal close together) and spread over a square
  lattice in a boustrophedon (snake) walk, with a free **routing channel** around
  every cell. Cells are partitioned across the two dies by splitting that order in
  half: the earlier half on the bottom die, the later half on the top die. Signals
  crossing the fold become **cross-die nets** that must use vias -- the 3D part of
  the problem falls out of the real dataflow, at a low-connectivity cut.

Feasibility is certified exactly like the generated tiers: route with the
negotiated-congestion router and validate with the independent checker, widening
the routing channel (a roomier grid) until a legal reference solution exists. That
reference is the tier's scoring baseline.

BLIF is parsed with the standard library only; only the connectivity (the
``.names`` headers, ``.inputs``, ``.outputs``, ``.latch``) is used -- the truth
tables are ignored, since the challenge is about routing, not logic.
"""
from __future__ import annotations

import json
import math
import os
import time
from dataclasses import dataclass, asdict
from typing import Dict, List, Optional, Tuple

from .checker import check
from .generator import GenResult, _PlacementError
from .model import (BOTTOM_DIE, TOP_DIE, Cell, Instance, Net, Pin, Submission,
                    layer_delay_profile)


# --------------------------------------------------------------------------- #
# BLIF parsing (connectivity only)
# --------------------------------------------------------------------------- #
@dataclass
class Netlist:
    model: str
    inputs: List[str]
    outputs: List[str]
    nodes: List[Tuple[str, List[str]]]   # (output_signal, [fanin_signals])
    latches: List[Tuple[str, str]]       # (output_signal, input_signal)


def parse_blif(text: str) -> Netlist:
    """Parse the connectivity of a (single-model) BLIF netlist.

    Line continuations (``\\`` at end of line) are joined; comments and truth
    tables are ignored. Only ``.model/.inputs/.outputs/.names/.latch`` matter.
    """
    text = text.replace("\\\n", " ")
    model = ""
    inputs: List[str] = []
    outputs: List[str] = []
    nodes: List[Tuple[str, List[str]]] = []
    latches: List[Tuple[str, str]] = []
    for line in text.splitlines():
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        if not s.startswith("."):
            continue  # a truth-table row for the current .names block
        parts = s.split()
        key = parts[0]
        if key == ".model":
            model = parts[1] if len(parts) > 1 else ""
        elif key == ".inputs":
            inputs.extend(parts[1:])
        elif key == ".outputs":
            outputs.extend(parts[1:])
        elif key == ".names":
            # .names <fanin...> <output>
            out = parts[-1]
            fins = parts[1:-1]
            nodes.append((out, fins))
        elif key == ".latch":
            # .latch <input> <output> [<type> <control>] [<init>]
            latches.append((parts[2], parts[1]))
        elif key == ".exdc":
            break  # don't parse the don't-care network
        # .end, .gate, .subckt, .default_input_arrival, ... : ignored
    return Netlist(model=model, inputs=inputs, outputs=outputs,
                   nodes=nodes, latches=latches)


# --------------------------------------------------------------------------- #
# Hypergraph: modules (cells-to-be) + signals (nets-to-be)
# --------------------------------------------------------------------------- #
@dataclass
class _Mod:
    mid: int
    kind: str                 # 'pi' | 'po' | 'gate' | 'const' | 'latch'
    drives: Optional[str]     # signal this module drives, or None
    reads: List[str]          # signals this module reads (in order)


def _build_modules(nl: Netlist) -> List[_Mod]:
    mods: List[_Mod] = []
    mid = 0

    # primary inputs: sources that drive their own signal
    for s in nl.inputs:
        mods.append(_Mod(mid, "pi", s, []))
        mid += 1
    # latches: a source (its output) that also reads its input
    for (out, inp) in nl.latches:
        mods.append(_Mod(mid, "latch", out, [inp]))
        mid += 1
    # logic nodes
    for (out, fins) in nl.nodes:
        kind = "const" if not fins else "gate"
        mods.append(_Mod(mid, kind, out, list(fins)))
        mid += 1

    # any signal that is read but never driven: synthesize a PI pad for it so
    # every net has a driver (well-formed BLIF won't need this, but be safe).
    driven = {m.drives for m in mods if m.drives is not None}
    read_sigs: List[str] = []
    seen = set()
    for m in mods:
        for s in m.reads:
            if s not in seen:
                seen.add(s)
                read_sigs.append(s)
    for s in nl.outputs:
        if s not in seen:
            seen.add(s)
            read_sigs.append(s)
    for s in read_sigs:
        if s not in driven:
            mods.append(_Mod(mid, "pi", s, []))
            driven.add(s)
            mid += 1

    # primary outputs: sinks that read their signal
    for s in nl.outputs:
        mods.append(_Mod(mid, "po", None, [s]))
        mid += 1
    return mods


def _module_adjacency(mods: List[_Mod], live: Dict[str, int]) -> Dict[int, set]:
    """Undirected module graph (star model): a signal's driver module is
    connected to every module that reads it. This is the connectivity placement
    must keep local."""
    driver_mod: Dict[str, int] = {}
    reader_mods: Dict[str, List[int]] = {}
    for m in mods:
        if m.drives is not None and m.drives in live:
            driver_mod[m.drives] = m.mid
        for s in m.reads:
            if s in live:
                reader_mods.setdefault(s, []).append(m.mid)
    adj: Dict[int, set] = {m.mid: set() for m in mods}
    for s, dm in driver_mod.items():
        for rm in reader_mods.get(s, ()):
            if rm != dm:
                adj[dm].add(rm)
                adj[rm].add(dm)
    return adj


def _rcm_order(ids: List[int], adj: Dict[int, set]) -> List[int]:
    """Reverse Cuthill-McKee ordering of ``ids``.

    RCM keeps graph-connected modules close together in the 1-D sequence (it
    minimizes the adjacency-matrix bandwidth). Packing that sequence onto the 2-D
    grid therefore keeps cells that share signals near each other, which is what
    makes the instance routable. Fully deterministic: BFS from the lowest-degree
    seed, visiting neighbors in ascending (degree, id) order, then reversed."""
    idset = set(ids)
    deg = {i: sum(1 for w in adj[i] if w in idset) for i in ids}
    visited = set()
    order: List[int] = []
    for start in sorted(ids, key=lambda i: (deg[i], i)):
        if start in visited:
            continue
        visited.add(start)
        q = [start]
        head = 0
        while head < len(q):
            u = q[head]
            head += 1
            nbrs = sorted((w for w in adj[u] if w in idset and w not in visited),
                          key=lambda w: (deg[w], w))
            for w in nbrs:
                visited.add(w)
                q.append(w)
        order.extend(q)
    order.reverse()
    return order


# --------------------------------------------------------------------------- #
# Instance construction
# --------------------------------------------------------------------------- #
@dataclass
class DesignConfig:
    name: str = "design"
    layers: int = 6
    center_delay: int = 1
    layer_slope: int = 1
    via_delay: int = 3
    channel: int = 5            # routing vertices between adjacent cells (roominess)
    seed: int = 0
    master_seed: int = 0
    max_grow: int = 8
    grow_step: int = 2          # channel widening per feasibility retry
    router: str = "negotiated"

    def to_params(self) -> Dict:
        return asdict(self)


def _footprint(npins: int) -> int:
    return max(1, math.ceil(math.sqrt(npins)))


def _live_signals(mods: List[_Mod]) -> Dict[str, int]:
    """Return {signal: n_readers} for signals that have a driver AND >=1 reader."""
    readers: Dict[str, int] = {}
    for m in mods:
        for s in m.reads:
            readers[s] = readers.get(s, 0) + 1
    driven = {m.drives for m in mods if m.drives is not None}
    return {s: c for s, c in readers.items() if s in driven and c >= 1}


def build_design_instance(name: str, nl: Netlist, cfg: DesignConfig,
                          channel: int) -> Instance:
    """Place a parsed netlist and return an Instance.

    Cells are spread uniformly over the grid on a square lattice with a ``slot``
    pitch of ``max_footprint + channel`` vertices, walked in RCM (snake) order so
    connectivity-adjacent cells sit on adjacent lattice points and every cell is
    surrounded by ``channel`` free routing tracks. A larger ``channel`` yields a
    roomier grid (used by the feasibility search to grow on failure)."""
    mods = _build_modules(nl)
    live = _live_signals(mods)
    by_id = {m.mid: m for m in mods}

    # which terminals of each module become pins (an ordered list of signals)
    def pin_signals(m: _Mod) -> List[str]:
        sigs: List[str] = []
        if m.drives is not None and m.drives in live:
            sigs.append(m.drives)       # slot 0 = driver terminal (if live)
        for s in m.reads:
            sigs.append(s)              # read terminals (all live: they have a driver)
        return sigs

    adj = _module_adjacency(mods, live)
    # connectivity-clustering order; keep only modules that carry pins
    placed_mods = [mid for mid in _rcm_order([m.mid for m in mods], adj)
                   if pin_signals(by_id[mid])]
    if not placed_mods:
        raise _PlacementError("netlist has no routable nets")

    # balance the two dies by cumulative pin count along the RCM order, so the
    # die "fold" lands at a low-connectivity cut and most nets stay intra-die.
    pin_counts = {mid: len(pin_signals(by_id[mid])) for mid in placed_mods}
    total_pins = sum(pin_counts.values())
    die: Dict[int, int] = {}
    acc = 0
    half = total_pins / 2.0
    for mid in placed_mods:
        die[mid] = BOTTOM_DIE if acc < half else TOP_DIE
        acc += pin_counts[mid]

    # square lattice sized for the busier die, with a routing channel per slot
    max_fp = max(_footprint(pin_counts[mid]) for mid in placed_mods)
    slot = max_fp + max(1, channel)
    n_bottom = sum(1 for mid in placed_mods if die[mid] == BOTTOM_DIE)
    n_top = len(placed_mods) - n_bottom
    cols = max(1, math.ceil(math.sqrt(max(n_bottom, n_top))))
    width = height = cols * slot

    cells: List[Cell] = []
    pins: List[Pin] = []
    driver_pin: Dict[str, int] = {}
    sink_pins: Dict[str, List[int]] = {}
    pid = 0

    def place_die(die_id: int, z: int, start_cid: int) -> int:
        nonlocal pid
        cid = start_cid
        seq = [mid for mid in placed_mods if die[mid] == die_id]
        for k, mid in enumerate(seq):
            r, c = divmod(k, cols)
            if r % 2 == 1:                 # boustrophedon: snake alternate rows
                c = cols - 1 - c
            ox, oy = c * slot, r * slot    # slot origin; cell hugs it, channel to +
            m = by_id[mid]
            sigs = pin_signals(m)
            side = _footprint(len(sigs))
            cells.append(Cell(id=cid, die=die_id, x=ox, y=oy, w=side, h=side))
            verts = [(vx, vy) for vy in range(oy, oy + side)
                     for vx in range(ox, ox + side)]
            for i, s in enumerate(sigs):
                vx, vy = verts[i]
                pins.append(Pin(id=pid, cell=cid, die=die_id, x=vx, y=vy, z=z))
                is_driver = (m.drives is not None and m.drives in live and i == 0)
                if is_driver:
                    driver_pin[s] = pid
                else:
                    sink_pins.setdefault(s, []).append(pid)
                pid += 1
            cid += 1
        return cid

    next_cid = place_die(BOTTOM_DIE, 0, 0)
    place_die(TOP_DIE, cfg.layers - 1, next_cid)

    # nets: one per live signal that has both a driver pin and >=1 sink pin
    nets: List[Net] = []
    for s in sorted(live):
        drv = driver_pin.get(s)
        snk = sink_pins.get(s, [])
        if drv is None or not snk:
            continue
        nets.append(Net(id=0, driver=drv, sinks=snk))
    nets.sort(key=lambda n: n.driver)
    for i, n in enumerate(nets):
        n.id = i

    inst = Instance(
        name=name,
        width=width, height=height, layers=cfg.layers,
        layer_delay=layer_delay_profile(cfg.layers, cfg.center_delay, cfg.layer_slope),
        via_delay=cfg.via_delay,
        cells=cells, pins=pins, nets=nets,
        params={**cfg.to_params(), "source": "blif", "design": name,
                "channel": channel, "n_modules": len(placed_mods)},
        seed=cfg.seed, master_seed=cfg.master_seed,
    )
    return inst


def generate_design_feasible(name: str, nl: Netlist, cfg: DesignConfig) -> GenResult:
    """Build a routing instance from a netlist and certify a legal reference by
    routing it and validating with the independent checker, widening the routing
    channel (a roomier grid) on failure."""
    from .negotiated import route_negotiated
    from . import baseline as _baseline

    channel = cfg.channel
    attempts = 0
    for _ in range(cfg.max_grow + 1):
        attempts += 1
        try:
            inst = build_design_instance(name, nl, cfg, channel)
        except _PlacementError:
            channel += cfg.grow_step
            continue
        if cfg.router == "negotiated":
            sub, _stats = route_negotiated(inst)
        else:
            sub, _stats = _baseline.route(inst)
        if sub is not None:
            res = check(inst, sub)
            if res.legal and res.total_delay is not None:
                return GenResult(instance=inst, reference=sub,
                                 baseline_total=res.total_delay, attempts=attempts)
        channel += cfg.grow_step
    raise RuntimeError(f"{name}: no feasible instance found after growth")


def load_netlist(path: str) -> Netlist:
    with open(path) as fh:
        return parse_blif(fh.read())


# --------------------------------------------------------------------------- #
# The released 'designs' tier (real EPFL circuits)
# --------------------------------------------------------------------------- #
DESIGN_DIR = "benchmarks_designs"
BLIF_DIR = os.path.join("designs", "blif")

# The released tier: real EPFL circuits, ordered easy -> hard by net count, each
# small enough that the (pure-Python) negotiated router certifies it in a few
# minutes. Larger vendored designs (dec.blif, cavlc.blif) ship too and can be
# turned into instances with ``m3d.cli import-design`` -- they are excluded from
# the released tier only because certifying them takes far longer here.
DESIGN_SPECS: List[Dict] = [
    {"name": "ctrl",      "blif": "ctrl.blif",      "channel": 5},
    {"name": "int2float", "blif": "int2float.blif", "channel": 5},
    {"name": "router",    "blif": "router.blif",    "channel": 5},
]


def design_configs(layers: int = 6, master_seed: int = 0) -> List[DesignConfig]:
    cfgs = []
    for i, spec in enumerate(DESIGN_SPECS):
        cfgs.append(DesignConfig(
            name=spec["name"], layers=layers, channel=spec.get("channel", 5),
            seed=master_seed + i, master_seed=master_seed, router="negotiated"))
    return cfgs


def _built_from(inst: Instance, cfg: DesignConfig) -> bool:
    """True if a stored instance was built from ``cfg`` (same layers, seeds,
    delay profile and router), so a resumed build may reuse it. The stored
    ``channel`` is the width the feasibility search settled on, which can exceed
    ``cfg.channel``, so it is not compared."""
    want = {k: v for k, v in cfg.to_params().items() if k != "channel"}
    have = inst.params or {}
    return (inst.layers == cfg.layers and inst.seed == cfg.seed
            and inst.master_seed == cfg.master_seed
            and all(have.get(k) == v for k, v in want.items()))


def build_design_suite(out_dir: str = DESIGN_DIR, blif_dir: str = BLIF_DIR,
                       layers: int = 6, master_seed: int = 0,
                       verbose: bool = True, resume: bool = True) -> Dict:
    """Build the 'designs' tier from the vendored BLIF netlists. Writes the same
    manifest/reference layout as the generated tiers, so the scorer, leaderboard
    and visualizer treat it identically."""
    ref_dir = os.path.join(out_dir, "reference")
    os.makedirs(ref_dir, exist_ok=True)
    cfgs = design_configs(layers=layers, master_seed=master_seed)

    # resume support: reuse a prior manifest's gen stats for cases we skip
    prev: Dict[str, Dict] = {}
    man_path = os.path.join(out_dir, "suite.json")
    if resume and os.path.exists(man_path):
        try:
            for c in json.load(open(man_path)).get("cases", []):
                prev[c["name"]] = c
        except Exception:
            prev = {}

    manifest = {"format": "m3d-suite", "tier": "designs", "master_seed": master_seed,
                "layers": layers, "n_cases": len(cfgs), "cases": []}
    for spec, cfg in zip(DESIGN_SPECS, cfgs):
        inst_file = f"{cfg.name}.json"
        ref_file = os.path.join("reference", f"{cfg.name}.sol.json")
        inst_path = os.path.join(out_dir, inst_file)
        ref_path = os.path.join(out_dir, ref_file)

        reused = False
        if resume and os.path.exists(inst_path) and os.path.exists(ref_path):
            try:          # trust it only if built from this config and it re-checks legal
                inst = Instance.load(inst_path)
                res = check(inst, Submission.load(ref_path))
                if (_built_from(inst, cfg) and res.legal
                        and res.total_delay is not None):
                    reused = True
                    base = res.total_delay
                    dt = prev.get(cfg.name, {}).get("gen_seconds", 0.0)
                    attempts = prev.get(cfg.name, {}).get("gen_attempts", 1)
            except Exception:
                reused = False

        if not reused:
            nl = load_netlist(os.path.join(blif_dir, spec["blif"]))
            t0 = time.time()
            result = generate_design_feasible(cfg.name, nl, cfg)
            dt = round(time.time() - t0, 2)
            inst = result.instance
            base = result.baseline_total
            attempts = result.attempts
            inst.save(inst_path)
            result.reference.save(ref_path)

        manifest["cases"].append({
            "name": cfg.name, "seed": inst.seed,
            "width": inst.width, "height": inst.height, "layers": inst.layers,
            "n_cells": len(inst.cells), "n_pins": len(inst.pins),
            "n_nets": len(inst.nets), "baseline_router": cfg.router,
            "baseline_total": base,
            "gen_attempts": attempts, "gen_seconds": dt,
            "instance_file": inst_file, "reference_file": ref_file,
            "source": "blif", "design": spec["blif"],
        })
        if verbose:
            tag = "reuse" if reused else "gen"
            print(f"  [designs/{tag}] {cfg.name}: {inst.width}x{inst.height}x{inst.layers}, "
                  f"{len(inst.nets)} nets, {len(inst.pins)} pins, "
                  f"baseline({cfg.router})={base}, {dt}s", flush=True)
        # write incrementally so a killed run still records finished cases
        with open(man_path, "w") as fh:
            json.dump(manifest, fh, indent=1)
    return manifest
