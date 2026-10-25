# cold_spt_lns

Independent cold-start routing by [kesudh](https://github.com/kesudh). The entry covers
all 45 cases across intro, hard, scale, stress, congested and designs. CI checks
submission structure, scoring files and legality. Acceptance and reproduction
remain maintainer decisions; winning requires leading the independent field
under the maintainers' judging rules.
The field comparison also checks the strongest complete timed public entry,
including derivatives, so raw scoreboard leadership is assessed separately.

## Algorithm and independence

Each invocation reads one benchmark instance and constructs its own initial
geometry. There is no published-solution input, trained artifact or embedded
route data. The constructor overwrites the output path before refinement reads
it, so refinement only receives geometry made in that same invocation.

The current source supports exact or weighted multi-goal A* with negotiated
present/history congestion prices, fanout-aware price normalization, optional
compact tree growth and continued global renegotiation after a legal layout. Exact per-net
SPT descent, fixed-count group LNS, negotiated repair and displacement chains
then improve that set. Portfolio mode runs mixed search followed by chain search
inside the same process. Joint-search variants generate alternate trees from
the current invocation and use branch and bound to choose a vertex-disjoint
combination for a small net group. That selection is exact only over its finite
candidate pool. All phases are timed.

The refinement implementation is adapted from kesudh's own earlier Rust engine.
Search code is reused; no previous entry's route geometry is an input. Finite
trial counts and trial-based cooling determine the result. Wall time controls
logging and measurement only. One solver thread is used.

The original producing version used two cold starts, six sweeps and 128 group
moves. Retained cases keep that source and configuration. Each case's archived
source version and arguments are authoritative; changing CLI defaults never
changes the provenance or runtime of a retained case. No global optimum or
leading score is claimed without a current field comparison.

## Check and reproduce

Requirements: Python 3.9+ standard library and Cargo/Rust with lockfile v4
support. Producing toolchain: Rust/Cargo 1.98.1, Windows 11.

From the repository root:

    cargo test --locked --manifest-path submissions/intro/cold_spt_lns/router/Cargo.toml
    python submissions/intro/cold_spt_lns/run.py --check-only
    python submissions/intro/cold_spt_lns/run.py --reproduce

The checker validates legal routes, delay totals, runtime records, input/output
hashes and the complete archived producing source. Reproduction builds that
exact version, supplies only the instance and recorded arguments, and requires
byte-identical output. Builds use an operating-system temporary cache to avoid
Windows linker path-length limits. Original runtimes are preserved.

To reproduce one case:

    python submissions/intro/cold_spt_lns/run.py --reproduce --tier hard --case case_01

To generate strict improvements from raw instances:

    python submissions/intro/cold_spt_lns/run.py --tier hard --refine-trials 12000 --refine-mode portfolio --only-improvements

Use --out-root PATH for separate candidates and --hardware to describe the
producing machine. Submitted routes are read only after the candidate finishes,
by the adoption checker. They never become solver inputs.

## Source versions and runtime

The immutable reproducer directory named by source_version in runs.json contains
the actual producing source and runner. Its source.json hashes every file. The
version identifier is SHA-256 of sorted relative-path:file-sha256 lines, each
terminated by LF. Per-case solver_args provide the complete command.

Archived revision `3f6f1614ded82a0ab8b416b4a1157bb27293c6ea2773eafca59901aefa015d3c`
adds optional exact whole-tree construction for small-fanout nets. Subset dynamic
programming minimizes the sum of root-to-sink physical delays plus nonnegative
congestion prices on the tree. Fanout and memory guards retain the existing
shortest-tree fallback. The producing arguments record when this option was
used; refinement and all cold starts remain inside the same timed invocation.
Its 31 Rust tests include independent exhaustive whole-tree objective checks.
The selected runs improve hard/case_02, case_05, case_06, case_07, case_08 and
case_09 by 246, 172, 326, 290, 226 and 92 delay units, respectively, against the
previous retained routes. Each complete producing configuration reproduced its
route bytes exactly; original producing times remain in runtime.json.

Archived revision `6ead40b0555c2ab96303a1aa2145097d2118058b02e100a552e23d5d4fdc4f56`
adds group candidates that avoid one neighboring net's individually preferred
tree at a time, before the existing all-neighbor and priced alternatives.
The same finite-pool selector then chooses mutually vertex-disjoint trees.
This broadens the candidate pool; it does not optimize over every legal tree.
Additional seeded extractions can change later proposals, so the producing
record pins the complete revision. Every candidate comes from the raw instance
and that invocation's own geometry. No saved route enters the producer.

Archived revision `44df9af614ff09212931d4168a42a64e0bf49657fe7b87155c6fb6dc0e71f696`
combines those yielding group candidates with the newer constructor archive.
The retained intro/case_06, case_07, case_10, case_11, case_12, case_14,
case_16, case_19 and case_20 recipes leave its optional mode-two constructor off.
Their full raw runs improve delay by 2, 4, 18, 18, 10, 40, 88, 22 and 28,
respectively, for 230 total. Full same-platform replays reproduce all nine
routes exactly; [yielding_replay_proofs.json](yielding_replay_proofs.json)
records their sources, arguments, hashes and separate replay times. Original
producing times remain in runtime.json. Intro/case_08 is refreshed separately
from the R10-safe source archive below: its delay is 7,519, 14 below the
previous retained route, with the original producing time preserved. The
hard/case_01, case_04, case_06 and case_07 routes improve delay by 26, 14, 60 and 84; the hard
certificate [yielding_replay_proofs.json](../../hard/cold_spt_lns/yielding_replay_proofs.json) records them.

Archived revision `350a84be0277f2acfbbce9d743fcecac23612c8cba87cc4dc5b2e302eb610842`
is the R10-safe intro/case_08 source archive ([manifest](reproducers/350a84be0277f2acfbbce9d743fcecac23612c8cba87cc4dc5b2e302eb610842/source.json)). Its full exact replay
reproduces the legal 7,519-delay route; the original producing runtime remains
authoritative, and the separate replay time is recorded in the [case certificate](yielding_replay_proofs.json).

Archived revision `78e62589eeac552696775443920d7679db5411d8990c36a9d4e241efec88bb70`
adds `--post-priority-trials` in that backend, defaulting to zero. After selecting
the invocation's best cold result and applying stable polish, it runs an additional
mixed refinement phase and retains a non-worsening final incumbent. Priority
proposals use up to eight nets and 32 priority-tree nodes; smaller groups retain
the existing finite-pool search. This independently adapts
[Ma et al. (2019)](https://arxiv.org/pdf/1812.06356) to static multi-sink trees.
The bounded priority search is incomplete and makes no optimality claim. Its
node costs order sibling exploration; they do not prune descendant states.
The entire added phase is included in the producing time.

Archived revision `13a645cca0c82dd4635900567546142ad611448cacb2d3dc70577a0c5fc2bbb7`
adds `--post-disjoint-trials` in that backend, defaulting to zero. After global
selection and stable polish, one additional conflict-search phase uses groups
of up to eight nets and at most 256 high-level nodes per proposal. Its queue
orders states by physical group delay plus 16 times the shared-vertex count;
a separate strict physical-delay cap controls acceptance. This heuristic queue
makes no optimality or bounded-suboptimality claim.

The search independently adapts [Li et al. (2019), disjoint splitting](https://ojs.aaai.org/index.php/ICAPS/article/download/3487/3355)
to static multi-sink tree membership. One branch prohibits a selected net from
using a contested vertex; the other requires its presence and prohibits every
other group member from using it. Required vertices become additional tree
terminals with zero objective demand. Delay still counts only the actual sinks,
so an actual-delay cap must not truncate the search to a distant required vertex.
Both positive and negative constraints enter the state key. Artificial non-pin
leaves are removed only after finding a complete compatible plan. The entire
cold construction, original search and added finishing phase share one producing
invocation and timer; saved routes are not producer inputs.

The current generator captures each source file's bytes once and installs
completed immutable files atomically with separate temporary files per writer.
Eight concurrent capture/validation processes passed without manifest races.

runtime.json measures one complete solver process: launch, input parsing, cold
construction, every internal refinement stage, output serialization, file close
and process exit. Compilation, pre-run instance copying and post-run official
checking are excluded. No producer times from another entry are substituted.

Most retained cases were produced on an Intel Core i7-13700HX (16 cores / 24
logical threads), 31.7 GiB RAM, Windows 11. The retained designs/int2float
case used an existing GCE Intel Xeon 2.20 GHz VM with four vCPUs, 15.6 GiB RAM,
Linux and Rust 1.90.0; exact-byte replay used that same platform and toolchain.
Every solver used one thread per case.
Other searches could be active concurrently. These are observed wall times
on the recorded hardware; per-case runs.json records identify the actual
platform, hardware, scheduling and producing invocation. Replay times do not
replace production times. Some Windows records disclose BelowNormal scheduling.

input_sha256 normalizes CRLF to LF for portable input comparison. The actual
producing bytes have a separate input_file_sha256. The solver reads the original
input. Source/output hashes use exact bytes preserved by .gitattributes.

## Validation

The primary router's twenty Rust tests cover exhaustive tiny-grid optima, full-Dijkstra differential
checks, shared-trunk objective accounting, entry pricing, direct-chain pricing
bounded conflict splitting, exhaustive compatible-candidate selection, and strict
repair success/rollback on crossing nets. Every adopted output also passes the official
independent checker. A compact-polish fixture checks equal-delay footprint
reduction, preservation of another net's tree, and stability on a second pass.
An independent simple-path oracle checks conflict search for groups of two
through eight nets, including fixed obstacles, infeasible cases and cost caps.
Both stock and bypass search are checked, including exhaustive branching trees
on tiny grids; the bypass oracle requires that helpful bypasses actually occur.
Conflict-priority probes are also checked with both search variants against
the path and branching-tree oracles, including infeasible cases and cost caps.
The archived priority backend has 23 Rust tests. Raw-case controller checks also
verify zero-budget output parity, a non-worsening enabled finish and exactly one
global finishing call after both normal and racing searches.
The archived weighted-disjoint backend has 25 Rust tests. Its independent
oracles cover positive membership constraints, multiple sinks, layered grids,
groups of two through eight nets, ownership restoration and weighted queues
at known tiny-instance optima. Raw controller checks verify default-zero byte
parity and a non-worsening single finishing call in normal and racing paths.


copy_audit.json describes the published revision's post-generation comparison
with pinned published Git blobs. It is refreshed when publishing improvements;
it does not automatically describe later uncommitted experiments. Other entries
are read by this audit after generation, never by the solver.

Maintainers control verification.json, reproduction status and final winners.

`--cbs-conflict-priority 1` optionally probes up to four shared vertices in the
most-overlapping route pair before a conflict split. Each probe computes the
exact single-net SPT after excluding that vertex. Conflicts that raise both
net bounds are preferred, followed by those raising either bound. This adapts
the conflict-priority idea in [ICBS](https://www.ijcai.org/Proceedings/15/Papers/110.pdf)
to static multi-sink routing. It samples conflicts rather than classifying all
of them. Both exclusion branches remain; it applies to stock and bypass search.
The default is0. `ccp=` reports the additional low-level probes, whose work is
included in the producing runtime. Node caps still count high-level expansions;
the option can spend more work per expansion and makes no general speed claim.

Two raw-instance pilots with60k main trials and1k six-net stock CBS trials gave
8846 versus8864 on hard01 and13473 versus13493 on hard04. Both remain worse than
the retained8828/13379 routes from different configurations. These observations
motivate further search, not a claim that this option wins a case or tier.
The bounded search can regress: a12k main/1k four-net stock pilot reached9054
with priority versus9046 without it. Adding1k bypass trials reached8998 with
priority versus9018 without it, with additional probe work. The default main
and stock-CBS outputs matched the previous source byte-for-byte; the enabled
stock-plus-bypass pilot also replayed byte-for-byte. Each configuration retains
its own producing time, rather than substituting the replay measurement.
This submission does not edit that file or infer a win from green CI.

## Research and algorithm credit

The implementation uses general routing concepts described by
[PathFinder](https://janders.eecg.utoronto.ca/1387_2015/readings/pathfinder.pdf)
and [FastRoute](https://doi.org/10.1155/2012/608362). Fanout-aware congestion
pricing and finite-pool joint selection were also informed by the publicly
disclosed [Luna router](https://github.com/IrwinJam/m3d-router/blob/81e4729b7c17c40987a8f57436fa94b9cb1ece96/router.py).
These additions are independently written here. No opponent source code or route
geometry was copied. Algorithmic similarity is not used as evidence of geometry
independence: the publication audit compares canonical route hashes against the
pinned main and open-PR Git blobs, and rejects matching non-exempt candidates.

An experimental `--cost-distance 1` constructor merges weighted sink components
using congestion cost plus sink-multiplicity-weighted path delay, then extracts a
physical root-distance tree from the embedded union. It is informed by
[Held and Perner (2025)](https://arxiv.org/html/2503.04419v1). The vertex-resource
adaptation is tested independently and claims no inherited approximation bound.
A full-Dijkstra differential test covers its weighted nearest-component search.

`--refine-each-start 1 --starts N` runs a complete internal refinement for each
fresh construction and selects the best final bytes. The producing runtime
includes every start and every refinement stage. `--refine-temperature`,
`--refine-slack` and `--refine-group-max` expose deterministic exploration
settings. `--refine-mode portfolio-cbs` uses bounded conflict-based search
followed by displacement chains; the conflict search has a fixed node budget
and makes no global optimality claim when that budget is exhausted.

`--compact-shortest-tree 1` adds greedy shared-branch candidates to the joint
pool. It chooses paths inside the exact physical shortest-path predecessor DAG,
so each sink keeps its shortest distance while the candidate tries to use fewer
vertices. It does not claim a minimum-size Steiner arborescence. A tiny-grid test
checks connectivity, sink distances, compactness and foreign-resource avoidance.

`--post-cbs-trials N` adds a conflict-search phase after the normal refinement,
using only this invocation's own best route. `--cbs-nodes` sets the fixed node
budget per conflict search (default 256). Every phase is included in producing
runtime, and the final phase retains a non-worsening incumbent.

`--joint-neutral 1` samples equal-delay, equal-footprint combinations in joint
search instead of always preserving the current geometry. Mode 2 also permits
larger footprints at equal delay; mode 0 preserves the original behavior.
Physical delay never increases in this joint move. A selection test checks
delay/footprint policy, tie diversity and outside-resource restoration.

`--penalized-astar 1` uses a physical-distance lower bound for congestion-priced
searches with up to eight sinks. Queue entries preserve the original floating
distance separately from priority, and the final equal-priority plateau is
completed before extraction. Differential tests include history, soft occupancy,
foreign pins and hard blockers. The default is 0 for compatibility.

In one matched 12,000-trial hard/case_01 pilot, this option preserved the exact
9,060 route bytes while observed full time fell from 50.886s to 32.995s. Search
expansions fell from 79,693,415 to 28,946,893 across the two phases. These are
single-case observations under concurrent workloads, not a general speed claim
or replacement timings for retained routes.

`--emit-global-pool 1` requires a separate `--out-root` and exports research
alternatives generated from the current invocation's own incumbent. The output
route is still independently checked. No external pool selector is part of this
producer, and offline research selection results are not eligible submissions.

`--retain-compact-best 1` retains a smaller tree when physical delay ties the
best seen delay. This lets the next phase and final polishing use the compact
geometry. It does not count footprint changes as delay improvements. The
default is 0 for compatibility with archived runs.

Experimental `--flow-pricing 1` divides a resource's congestion price by the
number of sink paths using it in this invocation's previous tree. Mode 2 uses
the square root of that count. An unused resource has count one, internal sinks
contribute to the count, and each net resets the counts before rerouting. This
replaces the global fanout divisor for root-SPT construction only. It is a
heuristic for the difference between per-tree resource use and per-sink delay;
there is no convergence or approximation guarantee. Default 0 preserves the
original pricing, and a fresh uniform fallback disables flow pricing. A test
checks branching, internal sinks and reset between nets. Pilots have mixed
results and do not establish a general quality improvement.

`--joint-group-max 6` extends joint candidate generation and compatible
selection to six interfering nets. The default remains four. Selection is
exact over its finite pool, with no claim of global tree optimality. The
exhaustive selector test covers two through six nets and fixed outside
resources. Larger groups can take more work. A matched 12k hard01 pilot gave
9040 for six nets versus9086 for four (43.625s versus42.538s observed), still
above the retained8866; broader experiments are needed.

`--cbs-group-max N` separately bounds the number of nets in a conflict-search
neighborhood. It defaults to4 and supports2 through8. The group still obeys
`--refine-group-max`, and `--cbs-nodes` bounds search expansions. Larger groups
permit coordinated changes involving more nets, but can require more search
work. A budget-limited failure is not proof that no improving route exists.
The larger-group oracle test enumerates all simple paths on small positive-cost
single-sink grids independently of the production search. Experimental matched
runs compare four and six nets; no general quality improvement is assumed.

Matched60k mixed-joint plus1k conflict-search pilots had identical trajectories
before the conflict phase. Four/six-net caps gave8856/8864 on hard01 and
13515/13493 on hard04, with observed full runtimes293.900/364.139s and
566.901/710.238s. This is mixed quality evidence under fixed node/trial budgets;
neither six-net result improves its retained submission route. The four-net
control also preserved the original8856 route bytes across the queue/cache
changes. Default4 is retained.

`--post-cbs-bypass-trials N` optionally adds N conflict-search trials after the
main search and the existing stock post-CBS phase. It defaults to0. The extra
phase uses helpful bypasses informed by [ICBS](https://www.ijcai.org/Proceedings/15/Papers/110.pdf):
an equal-cost alternative must reduce shared-vertex conflicts while satisfying
the parent's constraints. Its temporary child ban is discarded, and a discarded
child is not marked as searched. This is a static multi-sink adaptation; no
MAPF performance bound is inherited. Each expansion still counts against
`--cbs-nodes`. Exhausting that limit does not prove infeasibility.

The extra phase retains the completed stock incumbent, so its reported delay
cannot rise for the otherwise identical invocation. All extra work is timed.
Restart racing disables it on pilots and reserves it for finalists. It requires
positive main refinement trials and, when racing, a pilot shorter than the final
budget. Its added trial count is N per refined start, or N per promoted finalist.
This does not establish an advantage over spending the same work on stock CBS.
Matched total-trial controls and fresh raw-only producing runs are required to
assess that question; different phase boundaries also change cooling/polishing.

A raw hard01 pilot preserved the default9086 bytes. Adding1k stock trials reached
9046;2k stock trials reached9024;1k stock plus1k bypass reached9018 and replayed
byte-identically. Both extended configurations used14k total trials. This
supports that combined small configuration, not an isolated bypass effect or a
general speed guarantee. A larger eight-pilot comparison reached8832 with2k
stock trials per finalist and8828 with1k stock plus1k bypass; the latter ties
the retained8828 delay and has not improved that submission case.

A subsequent cold invocation using60k mixed-joint trials with the six-net
option, followed by1k conflict-search trials, reached8856 in293.355s. Its
mixed-joint phase ended at8866 and the last improvement came during conflict
search. This supports the combined configuration, not an isolated claim about
neighborhood size. The previous8866 configuration used different work and
settings, so its runtime is not a matched speed comparison.

`--penalized-integer-queue 1` uses integer queues for direct congestion prices
that are exact multiples of one quarter. It scales priorities by four, checks
a conservative u32 bound, and uses Dial buckets or a radix fallback for wider
costs. General floating history costs keep the original heap. Costs, sink
distances and tight-edge extraction stay in their original arithmetic. Tests
compare extracted trees with the floating heap, including entry prices, A*,
non-quarter costs, wide costs and overflow fallback. The default is 0.

One matched12k hard01 pair produced identical9086 route bytes and expansion
counts, with observed complete runtime43.526s versus40.488s for the option.
That is a single observation under concurrent workloads, not a general speed
claim. It does not replace timings for other retained runs.

A matched12k designs/router pair also produced identical76143 route bytes:
372.400s with the floating heap versus341.783s with the option. These are
observations under concurrent workloads, with original producing times kept.

`--penalized-heuristic-cache 1` caches a vertex's A* potential during one priced
search, using the same search stamps as its distance. Goals and prices are fixed
throughout that search. The default is 0. Differential tests cover the cached
search alongside the integer queues. A matched hard01 pair preserved exact9086
route bytes and expansion counts while reducing potential evaluations from
61,665,743 to41,515,426. Observed full runtimes were32.057s and30.768s under
concurrent workloads; this is not a general speed guarantee.

`--compact-polish-passes N` optionally runs bounded deterministic sweeps before
and after each refinement phase. With other nets fixed, it compares the current
tree with exact physical shortest-path candidates, including shared-branch DAG
extraction. A replacement must improve (sum of sink delays, number of vertices)
lexicographically. Each sweep restores ownership before the next net, and all
work is inside the producing runtime. N defaults to0 and is capped at64 per
boundary. Later search trajectories can still differ: matched pilots improved
designs/ctrl from53268 to53050 but worsened hard01 from9086 to9168. It is an
experimental option, not a uniform quality guarantee. Different configurations
may independently produce stronger candidates for the submitted portfolio.

`--refine-race-pilot P --refine-race-keep K` optionally allocates restart work
adaptively. It requires `--refine-each-start 1`, K no larger than the number of
refinement walks (`--starts N` times `--refine-forks F`),
and P no larger than `--refine-trials R`. Each start constructs fresh geometry
and receives P pilot trials. The best K successful pilots receive another
R-P trials with deterministic phase seeds. Post-CBS work is reserved for those
finalists. All construction, pilots, finalists and output work are timed in
one producing process. The controller retains its best completed pilot or
finalist; it imports no routes or learned artifacts. The maximum main-trial
count is N*F*P+K*(R-P), plus any explicitly configured post-CBS work.
When post-CBS work is enabled, P must be strictly smaller than R.

`--refine-forks F` defaults to1. With per-start refinement enabled, each cold
construction is serialized once and F independently seeded walks reset to those
same fresh bytes. It never reads an external route. This amortizes construction
work while exploring several refinement trajectories; it provides less initial
geometry diversity than N*F fresh constructions. Without pilot promotion the
main-trial count is N*F*R. With promotion all N*F pilots compete together. All
work is measured inside one invocation. The default preserves the previous seed
mapping. A raw-instance integration check compares default bytes with the prior
source, verifies incumbent retention across forks, and replays promoted walks.

This is a simple promotion stage inspired by adaptive resource allocation in
[Hyperband](https://arxiv.org/abs/1603.06560), not an implementation of its full
schedule or a transfer of its guarantees. Early pilot rankings can miss late
improvements. A four-start intro03 check with P=R=2000 preserved the uniform
control's exact1623 route bytes. P=500 and K=2 also reached1623, with5000 main
trials versus8000 in that control, and replayed byte-identically. Broader
equal-trial-budget comparisons are still required. Default P=0 disables racing.

Those intro03 pilots trail the currently retained1611 route; their role is to
check the allocation and replay path, not to establish a quality improvement.

`--final-stable-polish-passes N` optionally finishes this invocation's best
completed route with four deterministic shortest-path schedules. The schedules
combine forward/reverse net order with minimum/maximum vertex predecessor
tie-breaking. Each starts independently from the same internally generated
incumbent. A net replacement must not increase (sum of sink delays, vertex
count) lexicographically; equal-cost moves can release resources for later
nets. Other nets and foreign pins remain hard obstacles. Each schedule stops
after N sweeps or an unchanged complete geometry, and the best result is kept.
The default is0; enabled N is capped at64 per schedule. This stage runs once
after global restart selection, including the unrefined construction path.
All work and the final output write are inside the producing runtime. It has
no public warm-start input and requires no added dependencies.

An independent heap-Dijkstra reference test covers128 randomized multilayer
instances, and a branching fixture checks ownership and all four schedules.
Fresh raw hard01 checks preserve the default9086 route bytes, reach9080 with
two finishing sweeps, and replay byte-identically. A fresh unrefined intro03
check improves1697 to1681 and updates the optional pool's incumbent cost; a
restart-racing check confirms that finishing runs once after all finalists.
These are integration checks, not wins over the retained submission cases.
Separate experiments polishing saved own routes are research only: any
submitted replacement must come from a fresh, fully timed raw-input run.

The retained archived backend
`68b06974417ea78dd709578c7e06de87c4ea3e9204a24bb8bf92521705661c18`
adds `chain-polish-fast`. After a displacement chain is completely legal, it
exactly reroutes its affected nets against the other proposed and outside
routes for up to two sweeps. Each replacement must not increase that net's
physical sink-delay sum. The operator then accepts or rolls back the entire
chain using its aggregate physical delay. Original routes remain available
for rollback throughout. This adapts the exact reoptimization after
displacement described in the original
[Pathfinder LNS submission](https://github.com/partcleda/eda-3d-routing-challenge/pull/3)
to our own transactional chain implementation; no route geometry is imported.

`portfolio-joint-polish` divides each refinement allocation equally between
`mixed-joint` and `chain-polish-fast`. The retained hard/case_03 invocation uses
eight cold starts, an 8,000-trial pilot, two finalists and a 120,000-trial
allocation. Its 144,000 total mixed-joint trials match the previous producing
recipe's mixed-joint work, with another 144,000 chain trials added. Configured
post-CBS search and final stable polishing remain inside the same invocation.
The complete raw-input run produced delay 11,298 in 411.146 seconds, versus
the previous retained 11,394. Exact-source replay reproduced its bytes. This
supports the combined configuration; it is not an isolated operator or speed
claim. The producing record and original measured runtime are retained, and
the normal `--reproduce --tier hard --case case_03` command builds its pinned
backend from the immutable archive.


The archived backend
`f6532d33dcd611182daf06c34f888c341b38713fc0a007ff956a00d1f2d452be`
combines the same chain-polish implementation with a raw construction planner.
For cross-die single-sink nets, `--cross-profile-plan 2` jointly chooses among
four pin-derived profiles: either cheap middle plane and either X/Y elbow.
Profiles include their via columns. The planner estimates contention against
other profiles and independently generated pin-only trees, then performs at
most twelve deterministic coordinate passes. It reads no saved route geometry.

The selected profile supplies a soft off-profile cost of 256 in the relaxed
search's 1024-unit scale. Detailed routing can depart from it; strict repair,
refinement and the physical score do not include this guidance cost. Mode zero
skips the planner and preserves the previous constructor's random stream.
This is a construction heuristic, with no global-optimality guarantee.

The hard/case_03 producing configuration adds only profile mode two to the
previous 68b recipe: eight starts, 120,000-trial allocations, an 8,000-trial
pilot, two finalists, post-CBS and final stable polishing. The complete raw
run produced delay 11,242 in 353.200 seconds, versus the prior retained 11,298.
Its source archive, arguments, actual producing time and output hash are
pinned in the case record. This is evidence for the combined configuration;
the separate profile-only full run did not beat the retained chain-only route.
Exact-source standard replay reproduced the 11,242 route byte-for-byte.
