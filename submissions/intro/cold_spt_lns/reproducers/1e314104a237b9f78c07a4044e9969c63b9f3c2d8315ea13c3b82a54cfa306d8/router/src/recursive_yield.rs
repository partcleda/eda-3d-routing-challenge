//! Bounded recursive yield-profile closure and finite-column whole-layout selection.
//! This is a heuristic column generator; exactness is only over generated trees.
use super::*;
use std::collections::VecDeque;

const MAX_SPT_QUERIES: usize = 256;
const MAX_PROFILES_PER_NET: usize = 4;
const MAX_PROFILE_VERTEX_INCIDENCES: usize = 1_000_000;
const MAX_DFS_NODES: usize = 2_000_000;
const MAX_COMPONENT_NETS: usize = 512;

#[derive(Clone, Debug)]
struct Candidate {
    verts: Vec<u32>,
    edges: Vec<(u32, u32)>,
    delay: u64,
}

impl Candidate {
    fn canonicalize(&mut self) {
        self.verts.sort_unstable();
        self.verts.dedup();
        for edge in &mut self.edges {
            *edge = canon(edge.0, edge.1);
        }
        self.edges.sort_unstable();
        self.edges.dedup();
    }
}

#[derive(Default, Debug, Clone, Copy)]
struct ExpansionStats {
    queries: usize,
    added: usize,
    capped: bool,
}

fn profile_signature(candidate: &Candidate) -> &[(u32, u32)] {
    &candidate.edges
}

fn add_profile_for_net(
    pools: &mut [Vec<Candidate>],
    incidence_count: &mut usize,
    nid: usize,
    mut candidate: Candidate,
    profile_limit: usize,
) -> Result<bool, ()> {
    candidate.canonicalize();
    if pools[nid]
        .iter()
        .any(|old| profile_signature(old) == profile_signature(&candidate))
    {
        return Ok(false);
    }
    if pools[nid].len() >= profile_limit {
        return Err(());
    }
    let next = incidence_count
        .checked_add(candidate.verts.len())
        .ok_or(())?;
    if next > MAX_PROFILE_VERTEX_INCIDENCES {
        return Err(());
    }
    *incidence_count = next;
    pools[nid].push(candidate);
    Ok(true)
}

/// Expand only newly generated profiles. Each yield query blocks the full
/// target tree but ignores other incumbent owners, so newly found blockers can
/// recursively expose further profiles. The blocker relation is measured
/// against the original whole-route fallback only; this is not unrestricted
/// all-tree column generation.
fn expand_recursive_profiles<F>(
    pools: &mut [Vec<Candidate>],
    baseline_owner: &[i32],
    mut incidence_count: usize,
    mut queries_used: usize,
    mut queue: VecDeque<(usize, Candidate)>,
    query_limit: usize,
    profile_limit: usize,
    mut generate_yield: F,
) -> ExpansionStats
where
    F: FnMut(usize, &Candidate) -> Option<Candidate>,
{
    let mut stats = ExpansionStats {
        queries: queries_used,
        ..ExpansionStats::default()
    };
    while let Some((target_net, target)) = queue.pop_front() {
        let mut blockers = Vec::new();
        for &v in &target.verts {
            let owner = baseline_owner[v as usize];
            if owner >= 0 && owner as usize != target_net {
                blockers.push(owner as usize);
            }
        }
        blockers.sort_unstable();
        blockers.dedup();
        for blocker in blockers {
            if pools[blocker].len() >= profile_limit {
                stats.capped = true;
                continue;
            }
            if queries_used >= query_limit {
                stats.capped = true;
                queue.clear();
                break;
            }
            queries_used += 1;
            stats.queries = queries_used;
            let Some(candidate) = generate_yield(blocker, &target) else {
                continue;
            };
            match add_profile_for_net(pools, &mut incidence_count, blocker, candidate.clone(), profile_limit) {
                Ok(true) => {
                    stats.added += 1;
                    queue.push_back((blocker, candidate));
                }
                Ok(false) => {}
                Err(()) => stats.capped = true,
            }
        }
    }
    stats
}

/// Generate a blocker profile after two already-generated, mutually disjoint
/// profiles jointly vacate a common fallback owner. The first ordinary
/// recursive phase is deliberately limited to 128 SPT calls when this stage is
/// enabled; this phase and its follow-on expansion share the original 256-call
/// global cap. At most 256 profiles can be generated, so pair enumeration has
/// at most 32,640 profile pairs. Owner sets are derived only from stored tree
/// vertices (never by scanning the grid), with total copied owner incidences
/// bounded by MAX_PROFILE_VERTEX_INCIDENCES.
fn discover_pair_union_profiles<F>(
    pools: &mut [Vec<Candidate>],
    baseline_owner: &[i32],
    incidence_count: &mut usize,
    queries_used: &mut usize,
    mut generate_union_yield: F,
) -> (usize, usize, VecDeque<(usize, Candidate)>, bool)
where
    F: FnMut(usize, &[u32]) -> Option<Candidate>,
{
    let mut profiles = Vec::<(usize, usize, Vec<usize>)>::new();
    let mut owner_incidence_count = 0usize;
    for (nid, pool) in pools.iter().enumerate() {
        // Pools are still in insertion order here: slot zero is the unchanged
        // whole-tree fallback; every later entry was generated in this call.
        for (profile_index, candidate) in pool.iter().enumerate().skip(1) {
            let mut owners = Vec::new();
            for &v in &candidate.verts {
                let owner = baseline_owner[v as usize];
                if owner >= 0 && owner as usize != nid {
                    owners.push(owner as usize);
                }
            }
            owners.sort_unstable();
            owners.dedup();
            owner_incidence_count = owner_incidence_count.saturating_add(owners.len());
            if owner_incidence_count > MAX_PROFILE_VERTEX_INCIDENCES {
                return (0, 0, VecDeque::new(), true);
            }
            profiles.push((nid, profile_index, owners));
        }
    }
    let mut added = 0usize;
    let mut pair_queries = 0usize;
    let mut queue = VecDeque::new();
    let mut capped = false;
    'pairs: for i in 0..profiles.len() {
        let (left_nid, left_index, left_owners) = &profiles[i];
        // Own this bounded vertex list so pool insertion cannot overlap a borrow.
        let left_verts = pools[*left_nid][*left_index].verts.clone();
        for (right_nid, right_index, right_owners) in profiles.iter().skip(i + 1) {
            if left_nid == right_nid || vertices_overlap(&left_verts, &pools[*right_nid][*right_index].verts) {
                continue;
            }
            // Merge sorted fallback-owner lists and use the first shared owner
            // only, so a profile pair causes at most one target query.
            let mut li = 0usize;
            let mut ri = 0usize;
            let mut blocker = None;
            let mut capacity_refused = false;
            while li < left_owners.len() && ri < right_owners.len() {
                match left_owners[li].cmp(&right_owners[ri]) {
                    std::cmp::Ordering::Less => li += 1,
                    std::cmp::Ordering::Greater => ri += 1,
                    std::cmp::Ordering::Equal => {
                        let q = left_owners[li];
                        if q != *left_nid && q != *right_nid {
                            if pools[q].len() < MAX_PROFILES_PER_NET {
                                blocker = Some(q);
                                break;
                            }
                            capacity_refused = true;
                        }
                        li += 1;
                        ri += 1;
                    }
                }
            }
            let Some(blocker) = blocker else { capped |= capacity_refused; continue; };
            if *queries_used >= MAX_SPT_QUERIES {
                capped = true;
                break 'pairs;
            }
            let right_verts = &pools[*right_nid][*right_index].verts;
            let mut blocked = Vec::with_capacity(left_verts.len().saturating_add(right_verts.len()));
            let (mut a, mut b) = (0usize, 0usize);
            while a < left_verts.len() || b < right_verts.len() {
                let next = if b >= right_verts.len() || (a < left_verts.len() && left_verts[a] < right_verts[b]) {
                    let v = left_verts[a]; a += 1; v
                } else if a >= left_verts.len() || right_verts[b] < left_verts[a] {
                    let v = right_verts[b]; b += 1; v
                } else {
                    let v = left_verts[a]; a += 1; b += 1; v
                };
                blocked.push(next);
            }
            *queries_used += 1;
            pair_queries += 1;
            let Some(candidate) = generate_union_yield(blocker, &blocked) else { continue; };
            match add_profile_for_net(pools, incidence_count, blocker, candidate.clone(), MAX_PROFILES_PER_NET) {
                Ok(true) => { added += 1; queue.push_back((blocker, candidate)); }
                Ok(false) => {}
                Err(()) => capped = true,
            }
        }
    }
    (pair_queries, added, queue, capped)
}

fn vertices_overlap(a: &[u32], b: &[u32]) -> bool {
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] { return true; }
        if a[i] < b[j] { i += 1; } else { j += 1; }
    }
    false
}

#[derive(Default)]
struct UnionFind {
    parent: Vec<usize>,
}
impl UnionFind {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect() }
    }
    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            self.parent[hi] = lo;
        }
    }
}

fn select_component(
    pools: &[Vec<Candidate>],
    component: &[usize],
    fallback: &[usize],
    vertex_count: usize,
    remaining_nodes: usize,
) -> (Vec<usize>, usize, bool) {
    let mut order = component.to_vec();
    order.sort_by_key(|&nid| (pools[nid].len(), nid));
    let mut best = order.iter().map(|&n| fallback[n]).collect::<Vec<_>>();
    let mut best_cost = order.iter().map(|&n| pools[n][fallback[n]].delay).sum::<u64>();
    let options: Vec<Vec<usize>> = order.iter().map(|&nid| {
        let mut ids: Vec<usize> = (0..pools[nid].len()).collect();
        ids.sort_by_key(|&i| { let c = &pools[nid][i]; (c.delay, c.verts.len(), c.edges.clone()) });
        ids
    }).collect();
    let mut used = vec![false; vertex_count];
    let mut current = vec![0usize; order.len()];
    let mut nodes = 0usize;
    let mut capped = false;
    fn visit(
        depth: usize, order: &[usize], options: &[Vec<usize>], pools: &[Vec<Candidate>],
        used: &mut [bool], current: &mut [usize], cost: u64, best_cost: &mut u64,
        best: &mut Vec<usize>, nodes: &mut usize, node_limit: usize, capped: &mut bool,
    ) {
        if *nodes >= node_limit { *capped = true; return; }
        *nodes += 1;
        let lower = (depth..order.len())
            .map(|k| options[k].iter().map(|&i| pools[order[k]][i].delay).min().unwrap())
            .fold(0u64, u64::saturating_add);
        if cost.saturating_add(lower) >= *best_cost { return; }
        if depth == order.len() { *best_cost = cost; *best = current.to_vec(); return; }
        let nid = order[depth];
        for &ci in &options[depth] {
            let candidate = &pools[nid][ci];
            if candidate.verts.iter().any(|&v| used[v as usize]) { continue; }
            for &v in &candidate.verts { used[v as usize] = true; }
            current[depth] = ci;
            visit(depth + 1, order, options, pools, used, current,
                  cost.saturating_add(candidate.delay), best_cost, best,
                  nodes, node_limit, capped);
            for &v in &candidate.verts { used[v as usize] = false; }
            if *capped { return; }
        }
    }
    visit(0, &order, &options, pools, &mut used, &mut current, 0,
          &mut best_cost, &mut best, &mut nodes, remaining_nodes, &mut capped);
    let mut mapped = vec![0usize; component.len()];
    for (i, &nid) in order.iter().enumerate() {
        let slot = component.iter().position(|&x| x == nid).unwrap();
        mapped[slot] = best[i];
    }
    (mapped, nodes, capped)
}
fn select_finite_pool(
    pools: &[Vec<Candidate>],
    fallback: &[usize],
    vertex_count: usize,
) -> (Vec<usize>, usize, bool) {
    let mut uf = UnionFind::new(pools.len());
    let mut first = vec![usize::MAX; vertex_count];
    for (nid, pool) in pools.iter().enumerate() {
        for candidate in pool {
            for &v in &candidate.verts {
                let slot = &mut first[v as usize];
                if *slot == usize::MAX { *slot = nid; } else { uf.union(nid, *slot); }
            }
        }
    }
    let mut by_root = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for nid in 0..pools.len() {
        let root = uf.find(nid);
        by_root.entry(root).or_default().push(nid);
    }
    let mut selected = fallback.to_vec();
    let mut nodes = 0usize;
    let mut exact = true;
    for component in by_root.values() {
        if component.len() > MAX_COMPONENT_NETS {
            exact = false;
            continue;
        }
        if nodes >= MAX_DFS_NODES {
            exact = false;
            break;
        }
        let (choice, visited, capped) = select_component(
            pools,
            component,
            fallback,
            vertex_count,
            MAX_DFS_NODES - nodes,
        );
        nodes += visited;
        for (i, &nid) in component.iter().enumerate() { selected[nid] = choice[i]; }
        if capped { exact = false; break; }
    }
    (selected, nodes, exact)
}


const MAX_CLOSURE_FLOW_CALLS: usize = 2_048;
const CLOSURE_BINARY_PROJECTIONS: usize = 3;
#[derive(Default, Clone, Copy)]
struct ClosureStats { calls: usize, projections: usize, complete: bool, overflow: bool }

/// Supplement the full-pool DFS with up to three binary projections. Each net
/// offers its incumbent B tree or one ranked nonfallback column. The closure
/// relaxation handles alt_i intersecting B_j; branching resolves alt_i/alt_j
/// conflicts. It never replaces a better full-pool DFS assignment.
fn closure_supplement(
    pools: &[Vec<Candidate>], fallback: &[usize], selected: &mut [usize], vertex_count: usize,
) -> ClosureStats {
    use super::closure_select::solve_binary_projection;
    let mut stats = ClosureStats { complete: true, ..ClosureStats::default() };
    let mut uf = UnionFind::new(pools.len());
    let mut first = vec![usize::MAX; vertex_count];
    for (nid, pool) in pools.iter().enumerate() {
        for candidate in pool {
            for &v in &candidate.verts {
                let slot = &mut first[v as usize];
                if *slot == usize::MAX { *slot = nid; } else { uf.union(nid, *slot); }
            }
        }
    }
    let mut by_root = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for nid in 0..pools.len() { by_root.entry(uf.find(nid)).or_default().push(nid); }
    for component in by_root.values() {
        if component.len() > MAX_COMPONENT_NETS { stats.complete = false; continue; }
        for rank in 0..CLOSURE_BINARY_PROJECTIONS {
            if stats.calls >= MAX_CLOSURE_FLOW_CALLS { stats.complete = false; return stats; }
            let mut vars = Vec::<(usize, usize, i128)>::new();
            let mut var_for_net = vec![usize::MAX; pools.len()];
            for &nid in component {
                let mut alts: Vec<usize> = (0..pools[nid].len()).filter(|&i| i != fallback[nid]).collect();
                alts.sort_by_key(|&i| { let c = &pools[nid][i]; (c.delay, c.verts.len(), c.edges.clone()) });
                if let Some(&alt) = alts.get(rank) {
                    let delta = pools[nid][alt].delay as i128 - pools[nid][fallback[nid]].delay as i128;
                    var_for_net[nid] = vars.len();
                    vars.push((nid, alt, delta));
                }
            }
            if vars.is_empty() { continue; }
            stats.projections += 1;
            let mut base_owner = vec![usize::MAX; vertex_count];
            for &nid in component {
                for &v in &pools[nid][fallback[nid]].verts {
                    assert_eq!(base_owner[v as usize], usize::MAX, "fallback assignment must be legal");
                    base_owner[v as usize] = nid;
                }
            }
            let mut implications = Vec::new();
            let mut forced_false = vec![false; vars.len()];
            for (vi, &(nid, alt, _)) in vars.iter().enumerate() {
                for &v in &pools[nid][alt].verts {
                    let owner = base_owner[v as usize];
                    if owner == usize::MAX || owner == nid { continue; }
                    let blocker_var = var_for_net[owner];
                    if blocker_var == usize::MAX { forced_false[vi] = true; }
                    else { implications.push((vi, blocker_var)); }
                }
            }
            // A vertex shared by k alternatives contributes the full clique
            // of pairwise incompatibilities, not only a star from its first
            // owner. Sort compressed incidences, then enumerate each group.
            let incidence_count: usize = vars.iter().map(|&(nid, alt, _)| pools[nid][alt].verts.len()).sum();
            let mut alt_incidences = Vec::with_capacity(incidence_count);
            for (vi, &(nid, alt, _)) in vars.iter().enumerate() {
                for &v in &pools[nid][alt].verts { alt_incidences.push((v, vi)); }
            }
            alt_incidences.sort_unstable();
            // Store unique variable pairs as they are generated. With at
            // most 512 variables, this set is bounded by 512*511/2 even when
            // many distinct vertices induce the same conflict pair.
            let mut conflict_set = std::collections::BTreeSet::<(usize, usize)>::new();
            let mut start = 0usize;
            while start < alt_incidences.len() {
                let mut end = start + 1;
                while end < alt_incidences.len() && alt_incidences[end].0 == alt_incidences[start].0 { end += 1; }
                for i in start..end {
                    for j in i + 1..end {
                        let a = alt_incidences[i].1;
                        let b = alt_incidences[j].1;
                        if a != b { conflict_set.insert((a.min(b), a.max(b))); }
                    }
                }
                start = end;
            }
            let conflicts: Vec<(usize, usize)> = conflict_set.into_iter().collect();
            let deltas: Vec<i128> = vars.iter().map(|v| v.2).collect();
            let result = solve_binary_projection(
                &deltas, &implications, &conflicts, &forced_false,
                MAX_CLOSURE_FLOW_CALLS - stats.calls,
            );
            stats.calls += result.flow_calls;
            stats.complete &= result.complete;
            stats.overflow |= result.overflow;
            let base_cost: u64 = component.iter().map(|&nid| pools[nid][selected[nid]].delay).sum();
            let mut closure_cost = 0u64;
            let mut proposed: Vec<(usize, usize)> = component.iter().map(|&nid| (nid, fallback[nid])).collect();
            for (vi, yes) in result.selected.iter().enumerate() {
                if *yes {
                    let (nid, alt, _) = vars[vi];
                    let slot = proposed.iter_mut().find(|x| x.0 == nid).unwrap();
                    slot.1 = alt;
                }
            }
            for &(nid, ci) in &proposed { closure_cost = closure_cost.saturating_add(pools[nid][ci].delay); }
            if closure_cost < base_cost {
                for (nid, ci) in proposed { selected[nid] = ci; }
            }
            if !result.complete || result.overflow { return stats; }
        }
    }
    stats
}

#[derive(Default, Clone, Copy, Debug)]
struct DomainPropagationStats {
    fixed_nets: usize,
    removed_columns: usize,
    variable_nets: usize,
    max_nontrivial_component: usize,
}

/// Remove alternatives that conflict with a net whose only remaining choice
/// is its incumbent fallback. The fallback assignment is known legal, so a
/// forced fallback cannot be displaced by any globally feasible assignment.
/// The caller invokes this only after all discovery is complete and before
/// sorting, with fallback index zero in every pool.
fn propagate_fallback_domains(
    pools: &mut [Vec<Candidate>],
    fallback: &[usize],
    vertex_count: usize,
) -> DomainPropagationStats {
    assert_eq!(pools.len(), fallback.len());
    let nnet = pools.len();
    let mut fallback_owner = vec![usize::MAX; vertex_count];
    for (nid, &ci) in fallback.iter().enumerate() {
        assert_eq!(ci, 0, "propagation must run before pool sorting");
        assert!(!pools[nid].is_empty(), "fallback domain must be nonempty");
        for &v in &pools[nid][ci].verts {
            let slot = &mut fallback_owner[v as usize];
            assert_eq!(*slot, usize::MAX, "fallback assignment must be legal");
            *slot = nid;
        }
    }

    // Build a compressed vertex-to-alternative incidence index once. Each
    // incidence is inspected at most once when its vertex belongs to a
    // forced fallback, giving O(grid + total profile incidences) work.
    let mut counts = vec![0usize; vertex_count];
    let mut incidence_total = 0usize;
    for pool in pools.iter() {
        for candidate in pool.iter().skip(1) {
            for &v in &candidate.verts {
                let v = v as usize;
                assert!(v < vertex_count, "candidate vertex outside grid");
                counts[v] = counts[v].checked_add(1).expect("incidence count overflow");
                incidence_total = incidence_total.checked_add(1).expect("incidence total overflow");
            }
        }
    }
    assert!(incidence_total <= MAX_PROFILE_VERTEX_INCIDENCES,
            "propagation incidence cap exceeded");
    let mut offsets = vec![0usize; vertex_count + 1];
    for v in 0..vertex_count {
        offsets[v + 1] = offsets[v].checked_add(counts[v]).expect("incidence prefix overflow");
    }
    assert_eq!(offsets[vertex_count], incidence_total);
    let mut incidences = vec![(0usize, 0usize); incidence_total];
    let mut cursor = offsets[..vertex_count].to_vec();
    for (nid, pool) in pools.iter().enumerate() {
        for (ci, candidate) in pool.iter().enumerate().skip(1) {
            for &raw_v in &candidate.verts {
                let v = raw_v as usize;
                let at = cursor[v];
                incidences[at] = (nid, ci);
                cursor[v] += 1;
            }
        }
    }

    let mut active = pools.iter().map(|p| vec![true; p.len()]).collect::<Vec<_>>();
    let mut domain_size = pools.iter().map(Vec::len).collect::<Vec<_>>();
    let mut processed = vec![false; nnet];
    let mut queue = VecDeque::new();
    for nid in 0..nnet {
        if domain_size[nid] == 1 { queue.push_back(nid); }
    }
    let mut stats = DomainPropagationStats::default();
    while let Some(fixed_nid) = queue.pop_front() {
        if processed[fixed_nid] || domain_size[fixed_nid] != 1 { continue; }
        processed[fixed_nid] = true;
        stats.fixed_nets += 1;
        for &raw_v in &pools[fixed_nid][0].verts {
            let v = raw_v as usize;
            for &(other_nid, ci) in &incidences[offsets[v]..offsets[v + 1]] {
                if other_nid == fixed_nid || !active[other_nid][ci] { continue; }
                assert_ne!(ci, 0, "fallback columns are never removed");
                active[other_nid][ci] = false;
                domain_size[other_nid] -= 1;
                stats.removed_columns += 1;
                assert!(domain_size[other_nid] > 0, "safe propagation emptied a domain");
                if domain_size[other_nid] == 1 { queue.push_back(other_nid); }
            }
        }
    }
    for nid in 0..nnet {
        assert!(active[nid][0], "incumbent fallback must remain active");
        assert_eq!(domain_size[nid], active[nid].iter().filter(|&&yes| yes).count());
        pools[nid] = std::mem::take(&mut pools[nid])
            .into_iter().enumerate().filter_map(|(ci, c)| active[nid][ci].then_some(c)).collect();
        if pools[nid].len() > 1 { stats.variable_nets += 1; }
    }

    // Recompute the actual conflict components over the surviving columns.
    let mut uf = UnionFind::new(nnet);
    let mut first = vec![usize::MAX; vertex_count];
    for (nid, pool) in pools.iter().enumerate() {
        for candidate in pool {
            for &raw_v in &candidate.verts {
                let slot = &mut first[raw_v as usize];
                if *slot == usize::MAX { *slot = nid; } else { uf.union(nid, *slot); }
            }
        }
    }
    let mut component_sizes = vec![0usize; nnet];
    let mut component_has_variable = vec![false; nnet];
    for nid in 0..nnet {
        let root = uf.find(nid);
        component_sizes[root] += 1;
        component_has_variable[root] |= pools[nid].len() > 1;
    }
    stats.max_nontrivial_component = (0..nnet)
        .filter(|&root| component_has_variable[root])
        .map(|root| component_sizes[root])
        .max().unwrap_or(0);

    // Every nonfallback column came from at most one successful SPT query,
    // and the entire pool uses the same 256-query cap. Forced fallback nets
    // have no alternatives and no other net's alternative can touch their
    // fallback vertices, so they are isolated in this graph. Therefore each
    // remaining nontrivial component has at most 256 nets and cannot trip
    // the existing 512-net component guard.
    assert!(stats.variable_nets <= MAX_SPT_QUERIES,
            "nontrivial domains exceed the global query cap");
    assert!(stats.max_nontrivial_component <= MAX_SPT_QUERIES,
            "recomputed nontrivial component exceeds the global query cap");
    stats
}

fn raw_spt(inst: &Inst, ws: &mut Scratch, nid: usize, blocked_profile: Option<&Candidate>) -> Option<Candidate> {
    let blocked = blocked_profile.map(|p| p.verts.as_slice()).unwrap_or(&[]);
    raw_spt_blocked(inst, ws, nid, blocked, blocked_profile.is_none())
}

fn raw_spt_blocked(
    inst: &Inst,
    ws: &mut Scratch,
    nid: usize,
    blocked_vertices: &[u32],
    pins_only: bool,
) -> Option<Candidate> {
    let mut owner = vec![-1i32; inst.n as usize];
    for &v in blocked_vertices { owner[v as usize] = i32::MAX; }
    ws.dijkstra(inst, &owner, inst.nets[nid].driver, nid as i32, pins_only, u32::MAX);
    ws.extract_stable(inst, &inst.nets[nid]).map(|(verts, edges, delay)| Candidate { verts, edges, delay })
}

/// Called only by the explicit --recursive-yield-pool=1 opt-in after all cold
/// refinement and stable polishing. The original legal output is always the
/// fallback; write back only a separately revalidated strict physical-delay gain.
pub fn recursive_yield_pool_output(instance: &str, own_output: &str, use_closure_select: bool, use_pair_union_yield: bool, use_domain_propagation: bool) -> u64 {
    let mut inst = load_inst(instance);
    inst.early_stop = true;
    inst.use_radix = true;
    inst.use_dial = false;
    inst.use_astar = true;
    inst.free_dist = free_space_table(inst.w, inst.h, inst.l, &inst.ld, inst.vd);
    let original = load_solution(&inst, own_output).expect("own completed output must be legal");
    let nnet = inst.nets.len();
    let mut pools = Vec::with_capacity(nnet);
    
    let mut baseline_owner = vec![-1i32; inst.n as usize];
    let mut incidence_count = 0usize;
    for nid in 0..nnet {
        for &v in &original.verts[nid] {
            assert_eq!(baseline_owner[v as usize], -1, "incumbent must be legal");
            baseline_owner[v as usize] = nid as i32;
        }
        incidence_count = incidence_count.saturating_add(original.verts[nid].len());
        pools.push(vec![Candidate {
            verts: original.verts[nid].clone(),
            edges: original.edges[nid].clone(),
            delay: original.delay[nid],
        }]);
    }
    if incidence_count > MAX_PROFILE_VERTEX_INCIDENCES {
        println!("RECURSIVE_YIELD skipped=profile_incidence_cap baseline={}", original.total);
        return original.total;
    }
    let mut ws = Scratch::new(inst.n, nnet, 0);
    let mut queries = 0usize;
    let mut seed_capped = false;
    let mut seed_queue = VecDeque::new();
    let mut seed_order: Vec<usize> = (0..nnet).collect();
    seed_order.sort_by_key(|&nid| (std::cmp::Reverse(original.delay[nid]), nid));
    for nid in seed_order.into_iter().take(8) {
        if queries >= MAX_SPT_QUERIES { break; }
        queries += 1;
        if let Some(candidate) = raw_spt(&inst, &mut ws, nid, None) {
            match add_profile_for_net(
                &mut pools, &mut incidence_count, nid, candidate.clone(),
                if use_pair_union_yield { MAX_PROFILES_PER_NET - 1 } else { MAX_PROFILES_PER_NET },
            ) {
                Ok(true) => seed_queue.push_back((nid, candidate)),
                Ok(false) => {}
                Err(()) => { seed_capped = true; break; },
            }
        }
    }
    let ordinary_limit = if use_pair_union_yield { MAX_SPT_QUERIES / 2 } else { MAX_SPT_QUERIES };
    let mut stats = expand_recursive_profiles(
        &mut pools,
        &baseline_owner,
        incidence_count,
        queries,
        seed_queue,
        ordinary_limit,
        if use_pair_union_yield { MAX_PROFILES_PER_NET - 1 } else { MAX_PROFILES_PER_NET },
        |blocker, target| raw_spt(&inst, &mut ws, blocker, Some(target)),
    );
    stats.capped |= seed_capped;
    let mut pair_queries = 0usize;
    let mut pair_added = 0usize;
    if use_pair_union_yield {
        let mut incidence_count = pools.iter().map(|p| p.iter().map(|c| c.verts.len()).sum::<usize>()).sum::<usize>();
        let (q, added, pair_queue, pair_capped) = discover_pair_union_profiles(
            &mut pools,
            &baseline_owner,
            &mut incidence_count,
            &mut stats.queries,
            |blocker, blocked| raw_spt_blocked(&inst, &mut ws, blocker, blocked, false),
        );
        pair_queries = q;
        pair_added = added;
        stats.added += added;
        stats.capped |= pair_capped;
        let follow = expand_recursive_profiles(
            &mut pools,
            &baseline_owner,
            incidence_count,
            stats.queries,
            pair_queue,
            MAX_SPT_QUERIES,
            MAX_PROFILES_PER_NET,
            |blocker, target| raw_spt(&inst, &mut ws, blocker, Some(target)),
        );
        stats.queries = follow.queries;
        stats.added += follow.added;
        stats.capped |= follow.capped;
    }
    let propagation_stats = if use_domain_propagation {
        let fallback = vec![0usize; nnet];
        propagate_fallback_domains(&mut pools, &fallback, inst.n as usize)
    } else { DomainPropagationStats::default() };
    for pool in &mut pools {
        pool.sort_by_key(|c| (c.delay, c.verts.len(), c.edges.clone()));
    }
    let fallback: Vec<usize> = (0..nnet)
        .map(|nid| pools[nid].iter().position(|c| c.edges == original.edges[nid]).unwrap())
        .collect();
    let (choice, nodes, exact) = select_finite_pool(&pools, &fallback, inst.n as usize);
    let mut choice = choice;
    let closure_stats = if use_closure_select {
        closure_supplement(&pools, &fallback, &mut choice, inst.n as usize)
    } else { ClosureStats { complete: true, ..ClosureStats::default() } };
    let mut candidate = original.clone();
    for nid in 0..nnet {
        let selected = &pools[nid][choice[nid]];
        candidate.verts[nid] = selected.verts.clone();
        candidate.edges[nid] = selected.edges.clone();
        candidate.delay[nid] = selected.delay;
    }
    candidate.total = candidate.delay.iter().copied().sum();
    candidate.vcount = candidate.verts.iter().map(|v| v.len() as u64).sum();
    let tmp = format!("{own_output}.recursive-yield-check.tmp");
    let mut accepted = false;
    if candidate.total < original.total {
        write_sol(&inst, &candidate, &tmp).expect("write candidate validation file");
        if let Ok(checked) = load_solution(&inst, &tmp) {
            if checked.total == candidate.total && checked.total < original.total {
                write_sol(&inst, &checked, own_output).expect("write strict legal gain");
                accepted = true;
            }
        }
        let _ = std::fs::remove_file(&tmp);
    }
    println!(
        "RECURSIVE_YIELD queries={} added={} pair_queries={} pair_added={} profiles={} dfs_nodes={} pool_exact={} closure_capped={} selector_calls={} selector_projections={} selector_complete={} selector_overflow={} domain_fixed={} domain_removed={} domain_variable={} domain_max_component={} accepted={} before={} after={}",
        stats.queries, stats.added, pair_queries, pair_added,
        pools.iter().map(Vec::len).sum::<usize>(), nodes, exact, stats.capped,
        closure_stats.calls, closure_stats.projections, closure_stats.complete, closure_stats.overflow,
        propagation_stats.fixed_nets, propagation_stats.removed_columns,
        propagation_stats.variable_nets, propagation_stats.max_nontrivial_component,
        accepted, original.total, if accepted { candidate.total } else { original.total }
    );
    if accepted { candidate.total } else { original.total }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(verts: &[u32], delay: u64) -> Candidate {
        Candidate { verts: verts.to_vec(), edges: verts.windows(2).map(|w| (w[0], w[1])).collect(), delay }
    }
    fn feasible_costs(pools: &[Vec<Candidate>]) -> Vec<u64> {
        fn visit(pools: &[Vec<Candidate>], nid: usize, used: &mut Vec<u32>, cost: u64, out: &mut Vec<u64>) {
            if nid == pools.len() { out.push(cost); return; }
            for candidate in &pools[nid] {
                if candidate.verts.iter().any(|v| used.contains(v)) { continue; }
                used.extend_from_slice(&candidate.verts);
                visit(pools, nid + 1, used, cost + candidate.delay, out);
                used.truncate(used.len() - candidate.verts.len());
            }
        }
        let mut out = Vec::new();
        visit(pools, 0, &mut Vec::new(), 0, &mut out);
        out.sort_unstable();
        out
    }

    #[test]
    fn fallback_domain_propagation_removes_only_globally_impossible_columns() {
        let mut pools = vec![
            vec![c(&[0], 2)],
            vec![c(&[1], 3)],
            vec![c(&[2], 10), c(&[7], 9)],
            vec![c(&[3], 4), c(&[0, 4], 1)],
            vec![c(&[5], 5), c(&[3, 6], 1)],
            vec![c(&[8], 100), c(&[7, 9], 50)],
        ];
        let before = feasible_costs(&pools);
        assert!(!before.is_empty());
        let stats = propagate_fallback_domains(&mut pools, &[0; 6], 10);
        let after = feasible_costs(&pools);
        assert_eq!(before, after, "propagation must preserve every feasible assignment and its cost");
        assert_eq!(stats.fixed_nets, 4, "fixed net 0 forces net 3, then net 3 forces net 4");
        assert_eq!(stats.removed_columns, 2);
        assert_eq!(stats.variable_nets, 2);
        assert_eq!(stats.max_nontrivial_component, 2);
        assert_eq!(pools.iter().map(Vec::len).collect::<Vec<_>>(), vec![1, 1, 2, 1, 1, 2]);
    }

    #[test]
    fn fallback_domain_propagation_breaks_a_513_net_poison_component() {
        let fixed = 512usize;
        let mut pools = (0..fixed)
            .map(|v| vec![c(&[v as u32], 1)])
            .collect::<Vec<_>>();
        let mut poison = (0..=fixed).map(|v| v as u32).collect::<Vec<_>>();
        poison.sort_unstable();
        pools.push(vec![c(&[fixed as u32], 10), c(&poison, 20), c(&[(fixed + 1) as u32], 9)]);
        let fallback = vec![0usize; fixed + 1];

        let (old_choice, _, old_exact) = select_finite_pool(&pools, &fallback, fixed + 2);
        assert!(!old_exact, "unpropagated 513-net component exceeds the 512 component cap");
        assert_eq!(old_choice.iter().map(|&i| pools[0][i].delay).take(fixed).sum::<u64>()
                   + pools[fixed][old_choice[fixed]].delay, 522);
        let mut old_closure = fallback.clone();
        let old_closure_stats = closure_supplement(&pools, &fallback, &mut old_closure, fixed + 2);
        assert!(!old_closure_stats.complete);

        let stats = propagate_fallback_domains(&mut pools, &fallback, fixed + 2);
        assert_eq!(stats.fixed_nets, fixed);
        assert_eq!(stats.removed_columns, 1);
        assert_eq!(stats.variable_nets, 1);
        assert_eq!(stats.max_nontrivial_component, 1);
        assert_eq!(pools[fixed].len(), 2, "fallback and good alternative remain");
        assert_eq!(pools[fixed][0].delay, 10, "the incumbent fallback remains first");
        assert_eq!(pools[fixed][1].delay, 9, "only the poison alternative is removed");

        let (choice, _, exact) = select_finite_pool(&pools, &fallback, fixed + 2);
        assert!(exact);
        assert_eq!(choice[fixed], 1);
        let selected_cost = (0..fixed).map(|nid| pools[nid][choice[nid]].delay).sum::<u64>()
            + pools[fixed][choice[fixed]].delay;
        assert_eq!(selected_cost, 521);
        assert!(layout_is_vertex_disjoint(&pools, &choice));
        let mut closure_choice = fallback.clone();
        let closure_stats = closure_supplement(&pools, &fallback, &mut closure_choice, fixed + 2);
        assert!(closure_stats.complete);
        assert!(layout_is_vertex_disjoint(&pools, &closure_choice));
        assert_eq!((0..fixed).map(|nid| pools[nid][closure_choice[nid]].delay).sum::<u64>()
                   + pools[fixed][closure_choice[fixed]].delay, 521);
    }
    #[test]
    fn abstract_65_profile_cycle_is_reached_by_recursion() {
        let n = 65;
        let mut pools = (0..n).map(|i| vec![c(&[i as u32, (100 + i) as u32], 100)]).collect::<Vec<_>>();
        let mut owner = vec![-1i32; 200];
        for i in 0..n { owner[i] = i as i32; }
        let mut q = VecDeque::new();
        q.push_back((n - 1, c(&[0, 150], 1)));
        let stats = expand_recursive_profiles(&mut pools, &owner, n, 0, q, MAX_SPT_QUERIES, MAX_PROFILES_PER_NET, |blocker, _| {
            let next = (blocker + 1) % n;
            Some(c(&[next as u32, (100 + next) as u32], 1))
        });
        assert_eq!(stats.added, 65);
        assert_eq!(stats.queries, 66);
        assert_eq!(pools.iter().map(Vec::len).sum::<usize>(), n + 65);
    }
    #[test]
    fn recursive_closure_deduplicates_and_enforces_query_profile_and_vertex_caps() {
        let mut pools = vec![vec![c(&[0, 4], 10)], vec![c(&[1, 5], 10)], vec![c(&[2, 6], 10)]];
        let owner = vec![0, 1, 2, -1, 0, 1, 2];
        let mut q = VecDeque::new();
        q.push_back((2, c(&[0, 1], 1)));
        let stats = expand_recursive_profiles(&mut pools, &owner, 6, MAX_SPT_QUERIES - 1, q, MAX_SPT_QUERIES, MAX_PROFILES_PER_NET, |blocker, _| {
            Some(if blocker == 0 { c(&[0, 4], 10) } else { c(&[1, 5], 10) })
        });
        assert!(stats.capped);
        assert_eq!(stats.queries, MAX_SPT_QUERIES);
        assert_eq!(stats.added, 0);
        assert_eq!(pools.iter().map(Vec::len).sum::<usize>(), 3);
        let mut pools = vec![vec![c(&[1, 2], 10), c(&[1, 3], 11), c(&[1, 4], 12), c(&[1, 5], 13)], vec![c(&[6, 7], 10)]];
        let mut q = VecDeque::new();
        q.push_back((1, c(&[1, 9], 1)));
        let stats = expand_recursive_profiles(&mut pools, &[0, 0, 0, 0, 0, 0, 1, 1, -1, -1], 10, 0, q, MAX_SPT_QUERIES, MAX_PROFILES_PER_NET, |_, _| None);
        assert!(stats.capped);
        assert_eq!(stats.queries, 0);
        let mut one_pool = vec![vec![c(&[8, 9], 1)]];
        let mut incidences = MAX_PROFILE_VERTEX_INCIDENCES;
        assert!(add_profile_for_net(&mut one_pool, &mut incidences, 0, c(&[8, 10], 2), MAX_PROFILES_PER_NET).is_err());
        assert_eq!(incidences, MAX_PROFILE_VERTEX_INCIDENCES);
    }
    #[test]
    fn finite_selector_keeps_fallback_when_columns_conflict_and_finds_disjoint_gain() {
        let pools = vec![vec![c(&[0, 1], 10), c(&[0, 2], 5)], vec![c(&[3, 4], 10), c(&[2, 5], 4)]];
        let fallback = [0, 0];
        let (choice, _, exact) = select_finite_pool(&pools, &fallback, 6);
        assert!(exact);
        assert_eq!(choice, vec![0, 1]);
        assert_eq!(pools[0][choice[0]].delay + pools[1][choice[1]].delay, 14);
    }
    #[test]
    fn closure_supplement_finds_a_forced_three_net_cycle() {
        let pools = vec![
            vec![c(&[0], 10), c(&[4, 1], 5)],
            vec![c(&[1], 10), c(&[5, 2], 5)],
            vec![c(&[2], 10), c(&[6, 0], 5)],
        ];
        let fallback = [0, 0, 0];
        let mut selected = fallback.to_vec();
        let stats = closure_supplement(&pools, &fallback, &mut selected, 7);
        assert!(stats.complete && !stats.overflow);
        assert_eq!(selected, vec![1, 1, 1]);
        assert_eq!(selected.iter().enumerate().map(|(n, &i)| pools[n][i].delay).sum::<u64>(), 15);
    }
    #[test]
    fn closure_supplement_emits_full_conflict_clique_when_first_alt_is_forced_false() {
        let pools = vec![
            vec![c(&[0], 10), c(&[7, 4], 1)],
            vec![c(&[1], 10), c(&[7, 5], 1)],
            vec![c(&[2], 10), c(&[7, 6], 1)],
            vec![c(&[4], 10)],
        ];
        let fallback = [0, 0, 0, 0];
        let mut selected = fallback.to_vec();
        let stats = closure_supplement(&pools, &fallback, &mut selected, 8);
        assert!(stats.complete && !stats.overflow);
        assert_eq!(selected[0], 0, "first alternative is blocked by fixed fallback net 3");
        assert!(!(selected[1] == 1 && selected[2] == 1), "alternatives 1 and 2 share vertex 7");
        assert_eq!(selected.iter().enumerate().map(|(n, &i)| pools[n][i].delay).sum::<u64>(), 31);
    }
    #[test]
    fn closure_supplement_rejects_an_expensive_forced_blocker() {
        let pools = vec![
            vec![c(&[0], 10), c(&[1], 1)],
            vec![c(&[1], 10), c(&[2], 30)],
        ];
        let fallback = [0, 0];
        let mut selected = fallback.to_vec();
        let stats = closure_supplement(&pools, &fallback, &mut selected, 3);
        assert!(stats.complete && !stats.overflow);
        assert_eq!(selected, fallback);
    }
    #[test]
    fn bounded_component_search_keeps_the_legal_fallback_when_capped() {
        let pools = vec![vec![c(&[0, 1], 10), c(&[0, 2], 1)], vec![c(&[3, 4], 10), c(&[2, 5], 1)]];
        let fallback = [0, 0];
        let (choice, nodes, capped) = select_component(&pools, &[0, 1], &fallback, 6, 1);
        assert_eq!(nodes, 1);
        assert!(capped);
        assert_eq!(choice, fallback);
    }

    fn tiny_grid() -> Inst {
        let (w, h, l) = (13u32, 5u32, 1u32);
        let wh = w * h;
        let n = wh * l;
        let vid = |x: u32, y: u32| y * w + x;
        let nets = vec![
            Net { driver: vid(0, 0), sinks: vec![vid(2, 0)] },
            Net { driver: vid(10, 0), sinks: vec![vid(12, 0)] },
            Net { driver: vid(0, 2), sinks: vec![vid(12, 2)] },
            Net { driver: vid(6, 3), sinks: vec![vid(6, 4)] }, // fixed obstacle net
        ];
        let mut pin_owner = vec![-1i32; n as usize];
        for (nid, net) in nets.iter().enumerate() {
            pin_owner[net.driver as usize] = nid as i32;
            for &sink in &net.sinks { pin_owner[sink as usize] = nid as i32; }
        }
        Inst {
            early_stop: true, retain_negotiated: false, use_radix: false, use_dial: false,
            use_astar: false, budget_bound: false, free_dist: vec![0; n as usize],
            check_state: false, name: "authored-pair-union-grid".into(), w, h, l, wh, n,
            ld: vec![1], vd: 4, nets, pin_owner,
        }
    }

    fn route_candidate(inst: &Inst, path: &[u32]) -> Candidate {
        let mut candidate = Candidate {
            verts: path.to_vec(),
            edges: path.windows(2).map(|e| canon(e[0], e[1])).collect(),
            delay: path.windows(2).map(|e| edge_w(inst, e[0], e[1]).unwrap() as u64).sum(),
        };
        candidate.canonicalize();
        candidate
    }

    fn normalized_edges(candidate: &Candidate) -> Vec<(u32, u32)> {
        let mut edges = candidate.edges.clone();
        for edge in &mut edges { *edge = canon(edge.0, edge.1); }
        edges.sort_unstable();
        edges.dedup();
        edges
    }

    fn layout_cost(pools: &[Vec<Candidate>], selected: &[usize]) -> u64 {
        selected.iter().enumerate().map(|(nid, &ci)| pools[nid][ci].delay).sum()
    }

    fn assert_single_sink_candidate_legal(inst: &Inst, nid: usize, candidate: &Candidate) {
        assert!(candidate.verts.windows(2).all(|pair| pair[0] < pair[1]), "vertices must be sorted and unique");
        assert_eq!(candidate.edges.len() + 1, candidate.verts.len(), "single-sink tree must be acyclic");
        assert!(candidate.verts.contains(&inst.nets[nid].driver));
        assert!(inst.nets[nid].sinks.iter().all(|sink| candidate.verts.contains(sink)));
        for (v, &owner) in inst.pin_owner.iter().enumerate() {
            if owner >= 0 && owner != nid as i32 {
                assert!(!candidate.verts.contains(&(v as u32)), "candidate crossed foreign pin {v}");
            }
        }
        let mut cost = 0u64;
        for &(a, b) in &candidate.edges {
            assert!(candidate.verts.binary_search(&a).is_ok() && candidate.verts.binary_search(&b).is_ok());
            cost += edge_w(inst, a, b).expect("candidate edge must be a legal grid edge") as u64;
        }
        assert_eq!(cost, candidate.delay, "single-sink path delay equals its unique edge sum");
        let mut reached = vec![false; inst.n as usize];
        let mut stack = vec![inst.nets[nid].driver];
        reached[stack[0] as usize] = true;
        while let Some(v) = stack.pop() {
            for &(a, b) in &candidate.edges {
                let next = if a == v { Some(b) } else if b == v { Some(a) } else { None };
                if let Some(next) = next {
                    if !reached[next as usize] { reached[next as usize] = true; stack.push(next); }
                }
            }
        }
        assert!(inst.nets[nid].sinks.iter().all(|&sink| reached[sink as usize]));
    }

    fn layout_is_vertex_disjoint(pools: &[Vec<Candidate>], selected: &[usize]) -> bool {
        for i in 0..selected.len() {
            for j in i + 1..selected.len() {
                if vertices_overlap(&pools[i][selected[i]].verts, &pools[j][selected[j]].verts) { return false; }
            }
        }
        true
    }

    #[test]
    fn pair_union_real_grid_spt_and_whole_layout_find_strict_missing_column_gain() {
        let inst = tiny_grid();
        let vid = |x, y| y * inst.w + x;
        let x = vid(2, 2);
        let y = vid(10, 2);
        let mut ws = Scratch::new(inst.n, inst.nets.len(), 0);
        let c_base = raw_spt(&inst, &mut ws, 2, None).unwrap();
        let a_alt = route_candidate(&inst, &[vid(0,0),vid(1,0),vid(1,1),vid(1,2),x,vid(2,1),vid(2,0)]);
        let b_alt = route_candidate(&inst, &[vid(10,0),vid(10,1),y,vid(11,2),vid(11,1),vid(11,0),vid(12,0)]);
        assert_single_sink_candidate_legal(&inst, 0, &a_alt);
        assert_single_sink_candidate_legal(&inst, 1, &b_alt);
        assert_single_sink_candidate_legal(&inst, 2, &c_base);
        assert!(!vertices_overlap(&a_alt.verts, &b_alt.verts));
        for (v, &owner) in inst.pin_owner.iter().enumerate() {
            if owner >= 0 && owner != 0 { assert!(!a_alt.verts.contains(&(v as u32)), "A profile crossed a foreign pin"); }
            if owner >= 0 && owner != 1 { assert!(!b_alt.verts.contains(&(v as u32)), "B profile crossed a foreign pin"); }
        }
        let cx = raw_spt(&inst, &mut ws, 2, Some(&a_alt)).unwrap();
        let cy = raw_spt(&inst, &mut ws, 2, Some(&b_alt)).unwrap();
        assert!(c_base.verts.contains(&x) && c_base.verts.contains(&y));
        assert!(!cx.verts.contains(&x) && cx.verts.contains(&y));
        assert!(cy.verts.contains(&x) && !cy.verts.contains(&y));
        assert_single_sink_candidate_legal(&inst, 2, &cx);
        assert_single_sink_candidate_legal(&inst, 2, &cy);

        let a_fallback = route_candidate(&inst, &[vid(0,0),vid(0,1),vid(1,1),vid(2,1),vid(3,1),vid(4,1),vid(5,1),vid(5,0),vid(4,0),vid(3,0),vid(2,0)]);
        let b_fallback = route_candidate(&inst, &[vid(10,0),vid(9,0),vid(8,0),vid(7,0),vid(7,1),vid(8,1),vid(9,1),vid(10,1),vid(11,1),vid(12,1),vid(12,0)]);
        let fixed_obstacle = route_candidate(&inst, &[vid(6,3),vid(6,4)]);
        assert_single_sink_candidate_legal(&inst, 0, &a_fallback);
        assert_single_sink_candidate_legal(&inst, 1, &b_fallback);
        assert_single_sink_candidate_legal(&inst, 3, &fixed_obstacle);
        let mut pools = vec![
            vec![a_fallback, a_alt],
            vec![b_fallback, b_alt],
            vec![c_base.clone(), cx.clone(), cy.clone()],
            vec![fixed_obstacle],
        ];
        let fallback = [0usize; 4];
        let mut baseline_owner = vec![-1i32; inst.n as usize];
        for (nid, pool) in pools.iter().enumerate() {
            for &v in &pool[0].verts { baseline_owner[v as usize] = nid as i32; }
        }
        assert!(layout_is_vertex_disjoint(&pools, &fallback));
        let (old_choice, _, old_exact) = select_finite_pool(&pools, &fallback, inst.n as usize);
        assert!(old_exact && layout_is_vertex_disjoint(&pools, &old_choice));
        assert_eq!(layout_cost(&pools, &fallback), 33);
        assert_eq!(layout_cost(&pools, &old_choice), 31, "best whole layout with Cx/Cy only");
        assert_eq!((c_base.delay, cx.delay, cy.delay), (12, 14, 14));

        let mut incidences: usize = pools.iter().map(|p| p.iter().map(|c| c.verts.len()).sum::<usize>()).sum();
        let mut queries = 0usize;
        let (pair_queries, added, queue, capped) = discover_pair_union_profiles(
            &mut pools, &baseline_owner, &mut incidences, &mut queries,
            |blocker, blocked| raw_spt_blocked(&inst, &mut ws, blocker, blocked, false),
        );
        assert_eq!(pair_queries, 1, "only the disjoint A/B pair shares a different fallback blocker");
        assert_eq!(queries, 1);
        assert_eq!(added, 1);
        assert!(!capped);
        assert_eq!(queue.len(), 1);
        let cz_index = pools[2].iter().position(|candidate| candidate.delay == 16 && !candidate.verts.contains(&x) && !candidate.verts.contains(&y)).unwrap();
        let cz = &pools[2][cz_index];
        assert_single_sink_candidate_legal(&inst, 2, cz);
        assert_eq!(cz.edges.len() + 1, cz.verts.len());
        assert!(inst.nets[2].sinks.iter().all(|sink| cz.verts.contains(sink)));
        for (v, &owner) in inst.pin_owner.iter().enumerate() {
            if owner >= 0 && owner != 2 { assert!(!cz.verts.contains(&(v as u32)), "C SPT crossed a foreign pin"); }
        }
        assert_eq!((c_base.delay, cx.delay, cy.delay, cz.delay), (12, 14, 14, 16));
        assert_ne!(normalized_edges(&cx), normalized_edges(&cy));
        assert_ne!(normalized_edges(&cx), normalized_edges(cz));
        assert_ne!(normalized_edges(&cy), normalized_edges(cz));

        let (new_choice, _, new_exact) = select_finite_pool(&pools, &fallback, inst.n as usize);
        assert!(new_exact && layout_is_vertex_disjoint(&pools, &new_choice));
        assert_eq!(new_choice, vec![1, 1, cz_index, 0]);
        assert_eq!(layout_cost(&pools, &new_choice), 29);
        assert_eq!(layout_cost(&pools, &new_choice) - pools[3][0].delay, 28, "A/B/C traffic subtotal");
        let follow = expand_recursive_profiles(
            &mut pools, &baseline_owner, incidences, queries, queue, MAX_SPT_QUERIES, MAX_PROFILES_PER_NET,
            |blocker, target| raw_spt(&inst, &mut ws, blocker, Some(target)),
        );
        assert_eq!(follow.queries, 1, "pair-generated tree enters the ordinary recursive queue");
    }

    #[test]
    fn pair_union_mode_reserves_the_fourth_column_for_union_profiles() {
        let mut pools = vec![vec![c(&[0, 10], 10), c(&[1, 11], 9), c(&[2, 12], 8)]];
        let mut incidences = 6usize;
        assert!(add_profile_for_net(&mut pools, &mut incidences, 0, c(&[3, 13], 7), MAX_PROFILES_PER_NET - 1).is_err());
        assert_eq!(pools[0].len(), MAX_PROFILES_PER_NET - 1);
        assert_eq!(add_profile_for_net(&mut pools, &mut incidences, 0, c(&[4, 14], 6), MAX_PROFILES_PER_NET), Ok(true));
        assert_eq!(pools[0].len(), MAX_PROFILES_PER_NET);
    }

    #[test]
    fn pair_union_deduplicates_columns_and_rejects_overlapping_pairs() {
        let mut pools = vec![
            vec![c(&[0, 10], 9), c(&[2, 20], 3)],
            vec![c(&[1, 11], 9), c(&[3, 21], 3)],
            vec![c(&[4, 12], 9)],
        ];
        let owner = [0, 1, 2, 2, 2, -1, -1, -1, -1, -1, 0, 1, 2, 2, 2, -1, -1, -1, -1, -1, -1, -1];
        let mut incidences = 6usize;
        let mut queries = 0usize;
        let (q, added, queue, capped) = discover_pair_union_profiles(
            &mut pools, &owner, &mut incidences, &mut queries, |_, _| Some(c(&[4, 12], 9)),
        );
        assert_eq!(q, 1);
        assert_eq!(added, 0, "duplicate C column is not stored");
        assert!(queue.is_empty() && !capped);
        assert_eq!(pools[2].len(), 1);

        let mut overlapping = vec![vec![c(&[0], 9), c(&[2, 20], 3)], vec![c(&[1], 9), c(&[2, 21], 3)], vec![c(&[4], 9)]];
        let owner = [0, 1, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1];
        let mut count = 5usize;
        let mut used = 0usize;
        let (q, _, _, _) = discover_pair_union_profiles(&mut overlapping, &owner, &mut count, &mut used, |_, _| None);
        assert_eq!(q, 0, "a pair sharing a vertex is ineligible");

        let mut full = vec![
            vec![c(&[0], 9), c(&[2, 20], 3)],
            vec![c(&[1], 9), c(&[3, 21], 3)],
            vec![c(&[4], 9), c(&[5], 8), c(&[6], 7), c(&[7], 6)],
        ];
        let owner = [0, 1, 2, 2, 2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 0, 1];
        let mut count = 9usize;
        let mut used = 0usize;
        let (q, _, _, capped) = discover_pair_union_profiles(&mut full, &owner, &mut count, &mut used, |_, _| None);
        assert_eq!(q, 0, "a blocker already at the four-column cap is not queried");
        assert!(capped, "capacity refusal is visible");
    }

    #[test]
    fn pair_union_respects_the_existing_global_query_cap() {
        let mut pools = vec![
            vec![c(&[0], 9), c(&[2, 3, 20], 3)],
            vec![c(&[1], 9), c(&[4, 5, 21], 3)],
            vec![c(&[6], 9), c(&[7, 8, 22], 8)],
        ];
        let mut owner = vec![-1i32; 23];
        owner[2] = 1; owner[3] = 2; owner[20] = 0;
        owner[4] = 0; owner[5] = 2; owner[21] = 1;
        owner[6] = 0; owner[7] = 1; owner[22] = 2;
        let mut incidences = 12usize;
        let mut queries = MAX_SPT_QUERIES - 1;
        let (q, _, _, capped) = discover_pair_union_profiles(&mut pools, &owner, &mut incidences, &mut queries, |_, _| None);
        assert_eq!(q, 1);
        assert_eq!(queries, MAX_SPT_QUERIES);
        assert!(capped, "remaining eligible pairs are refused at the cap");
    }

}
