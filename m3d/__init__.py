"""M3D — a reproducible EDA routing challenge for a simplified monolithic-3D stack.

Two dies share a vertical stack of routing layers. Participants connect cell pins
across a 3D grid graph while minimizing an abstract additive routing delay.

Public modules:
    model      data model, delay profile, JSON I/O
    generator  deterministic, verified-feasible benchmark generator
    checker    independent legality checker + delay recomputation
    scorer     per-case scoring + normalized leaderboard
    baseline   reference router (Dijkstra trees + rip-up-and-reroute)
    suite      the generated benchmark tiers (intro, hard, scale, stress, congested)
    viz        matplotlib visualization
    cli        command-line entry point (``python -m m3d.cli``)
"""
from .model import (Instance, Submission, NetRoute, Cell, Pin, Net,
                    layer_delay_profile)
from .checker import check, CheckResult
from .scorer import score_case, leaderboard, CaseScore, Leaderboard

__all__ = [
    "Instance", "Submission", "NetRoute", "Cell", "Pin", "Net",
    "layer_delay_profile", "check", "CheckResult",
    "score_case", "leaderboard", "CaseScore", "Leaderboard",
]

__version__ = "1.0.0"
