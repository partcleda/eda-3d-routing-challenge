#!/usr/bin/env python3
"""Reject route files copied from another leaderboard entry.

A submission must contain routes its own router produced. A case whose routes
are identical (same edge set for every net, in any order) to another entry's
route file for that case is a copy, declared in `derived_from` or not. Cases
with fewer than SMALL_CASE_NETS nets are exempt: independent routers often
find the same routes there. CI runs this on the route files a PR adds or
changes, against the other entries of the same tier (its sibling
directories); given a directory, it checks every case of that entry.

Usage: python scripts/check_copies.py submissions/<tier>/<name>[/<case>.sol.json] [...]
"""
import hashlib
import os
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, REPO_ROOT)

from m3d.cli import _tier_dir_map, _load_manifest, _submission_entries  # noqa: E402
from m3d.model import Submission, canon_edge                           # noqa: E402

SMALL_CASE_NETS = 10


def route_hash(path: str):
    """A digest of the routes in a .sol.json, independent of edge and net order."""
    try:
        sub = Submission.load(path)
    except (OSError, ValueError):
        return None  # verify_submissions.py reports unreadable files
    nets = sorted((r.net, sorted(canon_edge(a, b) for a, b in r.edges)) for r in sub.routes)
    return hashlib.sha256(repr(nets).encode()).hexdigest()


def problems(path: str):
    """List the cases of one entry (a directory, or one of its route files)
    copied from another entry."""
    path = os.path.normpath(path)
    only = None
    if path.endswith(".sol.json"):
        path, only = os.path.split(path)
    sub_dir = path
    name = os.path.basename(sub_dir)
    troot = os.path.dirname(sub_dir)
    suite_dir = _tier_dir_map().get(os.path.basename(troot))
    if not suite_dir or not os.path.isdir(troot):
        return []
    if not os.path.isabs(suite_dir):
        suite_dir = os.path.join(REPO_ROOT, suite_dir)
    out = []
    others = [(n, d) for n, d in _submission_entries(troot) if n != name]
    for c in _load_manifest(suite_dir)["cases"]:
        if c.get("n_nets", SMALL_CASE_NETS) < SMALL_CASE_NETS:
            continue
        sol = f"{c['name']}.sol.json"
        if only and sol != only:
            continue
        mine = route_hash(os.path.join(sub_dir, sol))
        if mine is None:
            continue
        same = [n for n, d in others
                if os.path.exists(os.path.join(d, sol)) and route_hash(os.path.join(d, sol)) == mine]
        if same:
            out.append(f"{c['name']}: routes identical to {', '.join(same)}")
    return out


def main(paths) -> int:
    failed = False
    for d in paths:
        for p in problems(d):
            print(f"{d}: {p}")
            failed = True
    if failed:
        print("Copied route files are not accepted, even with derived_from; see "
              "CONTRIBUTING.md (Rules: copies).")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
