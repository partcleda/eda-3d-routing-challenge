//! Greedy marginal-cost rooted-tree construction (experimental, opt-in).
//!
//! For one currently selected sink occurrence group of multiplicity m, every
//! attachment from an existing tree vertex v has exact incremental objective
//!   m * (d_T(root,v) + len(Q)) * SCALE + sum(lambda[x] for x newly added).
//! Existing tree vertices are multi-source starts, so their prefix is charged
//! once per selected sink occurrence but their already-paid vertex prices are
//! not charged again. The search does not charge other pending sink occurrences
//! encountered en route; their root distances are included in the final whole-
//! tree score. Thus this is exact only for the selected group plus the already
//! processed sink set, not for the full set of still-pending sinks. Foreign
//! pins are hard obstacles.
//! This is a greedy tree-column heuristic, not an exact global arborescence.
use super::{Instance, Route, Search};
use std::cmp::Reverse;

pub(super) const SCALE: u64 = 1024;
/// Fixed aggregate work budget per cold `build` call. It counts source/relax
/// pushes and settled pops, bounding both graph work and heap growth.
pub(super) const MAX_WORK_PER_BUILD: usize = 1_000_000;

fn spend(budget: &mut usize) -> bool {
    if *budget == 0 { return false; }
    *budget -= 1;
    true
}

fn checked_price(lambda: &[u64], v: usize) -> Option<u128> {
    Some(lambda.get(v).copied()? as u128)
}

/// O(route vertices) common comparison score. Generated routes contain each
/// owned vertex once; this avoids allocating whole-grid validation scratch for
/// each of the two phase candidates.
pub(super) fn common_score(route: &Route, lambda: &[u64]) -> Option<u128> {
    route.vertices.iter().try_fold(
        (route.delay as u128).checked_mul(SCALE as u128)?,
        |sum, &v| sum.checked_add(*lambda.get(v)? as u128),
    )
}

/// Independently reconstruct rooted distances from the emitted edges, then
/// score every sink occurrence and each owned vertex toll exactly once.
pub(super) fn objective(inst: &Instance, nid: usize, route: &Route, lambda: &[u64]) -> Option<u128> {
    let net = inst.nets.get(nid)?;
    if route.vertices.is_empty() || route.edges.len().checked_add(1)? != route.vertices.len() {
        return None;
    }
    let mut present = Vec::new();
    present.try_reserve(inst.size).ok()?;
    present.resize(inst.size, false);
    for &v in &route.vertices {
        if v >= inst.size || present[v] || (inst.pins[v] >= 0 && inst.pins[v] != nid as i32) {
            return None;
        }
        present[v] = true;
    }
    if !present.get(net.driver).copied().unwrap_or(false) { return None; }
    let mut adjacency: Vec<Vec<(usize, u64)>> = Vec::new();
    adjacency.try_reserve(inst.size).ok()?;
    adjacency.resize_with(inst.size, Vec::new);
    for &(a, b) in &route.edges {
        if a >= inst.size || b >= inst.size || a == b || !present[a] || !present[b]
            || !inst.neighbors[a].iter().any(|&(v, _)| v == b)
        { return None; }
        let w = inst.weight(a, b);
        adjacency[a].try_reserve(1).ok()?;
        adjacency[b].try_reserve(1).ok()?;
        adjacency[a].push((b, w));
        adjacency[b].push((a, w));
    }
    let mut distance = Vec::new();
    distance.try_reserve(inst.size).ok()?;
    distance.resize(inst.size, None::<u64>);
    let mut queue = Vec::new();
    queue.try_reserve(route.vertices.len()).ok()?;
    distance[net.driver] = Some(0);
    queue.push(net.driver);
    let mut cursor = 0;
    while cursor < queue.len() {
        let u = queue[cursor];
        cursor += 1;
        let du = distance[u]?;
        for &(v, w) in &adjacency[u] {
            if distance[v].is_none() {
                distance[v] = Some(du.checked_add(w)?);
                queue.push(v);
            }
        }
    }
    if route.vertices.iter().any(|&v| distance[v].is_none()) { return None; }
    let delay = net.sinks.iter().try_fold(0u64, |sum, &sink| {
        sum.checked_add(distance.get(sink).copied().flatten()?)
    })?;
    if delay != route.delay { return None; }
    let mut total = (delay as u128).checked_mul(SCALE as u128)?;
    for &v in &route.vertices {
        total = total.checked_add(checked_price(lambda, v)?)?;
    }
    Some(total)
}

/// Construct one complete legal net tree under a fixed vertex-price snapshot.
/// `budget` is shared across all net attempts in one build invocation. Failure
/// (including a cap or overflow) leaves the caller to use its configured route.
pub(super) fn route(
    inst: &Instance,
    search: &mut Search,
    nid: usize,
    lambda: &[u64],
    budget: &mut usize,
) -> Option<Route> {
    route_with_direction(inst, search, nid, lambda, 0, budget)
}

/// As `route`, but add one phase's nonnegative orientation guidance to the
/// priced attachment searches. `direction_mode` is 0 (uniform), 1 (phase 0),
/// or 2 (phase 1). The final candidate comparator deliberately omits this
/// guidance and uses only the common physical-delay-plus-vertex-toll objective.
pub(super) fn route_with_direction(
    inst: &Instance,
    search: &mut Search,
    nid: usize,
    lambda: &[u64],
    direction_mode: u64,
    budget: &mut usize,
) -> Option<Route> {
    if direction_mode > 2 { return None; }
    let net = inst.nets.get(nid)?;
    // `build` preallocates this scratch buffer for the fast path, but keep the
    // kernel safe as a standalone API (including focused unit tests). Failure
    // is a normal candidate refusal; the caller can use its configured route.
    if search.marginal_dist.len() < inst.size {
        let additional = inst.size.checked_sub(search.marginal_dist.len())?;
        search.marginal_dist.try_reserve(additional).ok()?;
        search.marginal_dist.resize(inst.size, 0u128);
    }
    if direction_mode != 0 && search.marginal_tree_direction.len() < inst.size {
        let additional = inst.size.checked_sub(search.marginal_tree_direction.len())?;
        search.marginal_tree_direction.try_reserve(additional).ok()?;
        search.marginal_tree_direction.resize(inst.size, 0u64);
    }
    let root = net.driver;
    let tree_epoch = search.tree_epoch.checked_add(1)?;
    search.tree_epoch = tree_epoch;
    search.tree_stamp[root] = tree_epoch;
    search.tree_delay[root] = 0;
    if direction_mode != 0 { search.marginal_tree_direction[root] = 0; }
    let mut vertices = Vec::new();
    let mut edges = Vec::new();
    vertices.try_reserve(1).ok()?;
    vertices.push(root);

    // Repeated terminal IDs are one attachment with multiplicity m. Stable
    // farthest-first ordering avoids random draws and exposes shared trunks.
    let mut terminals = Vec::new();
    terminals.try_reserve(net.sinks.len()).ok()?;
    terminals.extend_from_slice(&net.sinks);
    terminals.sort_unstable();
    let mut grouped: Vec<(usize, u64)> = Vec::new();
    grouped.try_reserve(terminals.len()).ok()?;
    for sink in terminals {
        if let Some((last, multiplicity)) = grouped.last_mut() {
            if *last == sink {
                *multiplicity = multiplicity.checked_add(1)?;
                continue;
            }
        }
        grouped.push((sink, 1));
    }
    grouped.sort_by_key(|&(sink, _)| (Reverse(inst.lower_bound(root, &[sink])), sink));

    for (sink, multiplicity) in grouped {
        if search.tree_stamp[sink] == tree_epoch { continue; }
        let epoch = search.epoch.checked_add(1)?;
        search.epoch = epoch;
        search.marginal_queue.clear();
        let multiplier = multiplicity.checked_mul(SCALE)?;
        // Every current T vertex is a source with its already-paid root prefix.
        // Entering T again is forbidden, so a candidate attaches exactly once.
        for &u in &vertices {
            if !spend(budget) { return None; }
            let prefix_physical = (search.tree_delay[u] as u128).checked_mul(SCALE as u128)?;
            let prefix_guidance = if direction_mode == 0 { 0 } else { search.marginal_tree_direction[u] as u128 };
            let d = prefix_physical.checked_add(prefix_guidance)?
                .checked_mul(multiplicity as u128)?;
            search.stamp[u] = epoch;
            search.marginal_dist[u] = d;
            search.prev[u] = u;
            let h = (inst.lower_bound(u, &[sink]) as u128).checked_mul(multiplier as u128)?;
            let f = d.checked_add(h)?;
            search.marginal_queue.try_reserve(1).ok()?;
            search.marginal_queue.push(Reverse((f, u128::MAX - d, u, d)));
        }
        let mut reached = false;
        while let Some(Reverse((_, _, u, d))) = search.marginal_queue.pop() {
            if search.stamp[u] != epoch || search.marginal_dist[u] != d || search.done[u] == epoch { continue; }
            if !spend(budget) { return None; }
            search.done[u] = epoch;
            if u == sink { reached = true; break; }
            for &(v, w) in &inst.neighbors[u] {
                if search.tree_stamp[v] == tree_epoch
                    || (inst.pins[v] >= 0 && inst.pins[v] != nid as i32)
                { continue; }
                let edge_physical = (w as u128).checked_mul(SCALE as u128)?;
                // An orientation surcharge can be unrepresentable for one
                // edge (for example, `via * SCALE` overflowing). That edge
                // is unusable in this phase, but it must not abort the search:
                // another edge from this vertex may still lead to a legal
                // route with representable nonnegative guidance.
                let direction = if direction_mode == 0 {
                    0
                } else {
                    match super::lateral_direction_penalty(inst, u, v, direction_mode) {
                        Some(penalty) => penalty as u128,
                        None => continue,
                    }
                };
                let edge_guidance = edge_physical.checked_add(direction)?
                    .checked_mul(multiplicity as u128)?;
                let price = checked_price(lambda, v)?;
                let nd = d.checked_add(edge_guidance)?.checked_add(price)?;
                if search.done[v] != epoch && (search.stamp[v] != epoch || nd < search.marginal_dist[v]) {
                    if !spend(budget) { return None; }
                    search.stamp[v] = epoch;
                    search.marginal_dist[v] = nd;
                    search.prev[v] = u;
                    // Physical-only lower bound is consistent because all
                    // vertex prices are nonnegative.
                    let h = (inst.lower_bound(v, &[sink]) as u128).checked_mul(multiplier as u128)?;
                    let f = nd.checked_add(h)?;
                    search.marginal_queue.try_reserve(1).ok()?;
                    search.marginal_queue.push(Reverse((f, u128::MAX - nd, v, nd)));
                }
            }
        }
        if !reached { return None; }
        let mut path = Vec::new();
        let mut u = sink;
        while search.tree_stamp[u] != tree_epoch {
            if path.len() >= inst.size { return None; }
            path.try_reserve(1).ok()?;
            path.push(u);
            let p = search.prev[u];
            if p == u { return None; }
            u = p;
        }
        for v in path.into_iter().rev() {
            let w = inst.weight(u, v);
            search.tree_stamp[v] = tree_epoch;
            search.tree_delay[v] = search.tree_delay[u].checked_add(w)?;
            if direction_mode != 0 {
                let direction = super::lateral_direction_penalty(inst, u, v, direction_mode)?;
                search.marginal_tree_direction[v] = search.marginal_tree_direction[u].checked_add(direction)?;
            }
            if edges.try_reserve(1).is_err() || vertices.try_reserve(1).is_err() { return None; }
            edges.push((u.min(v), u.max(v)));
            vertices.push(v);
            u = v;
        }
    }
    let delay = net.sinks.iter().try_fold(0u64, |sum, &s| sum.checked_add(search.tree_delay[s]))?;
    edges.sort_unstable();
    edges.dedup();
    vertices.sort_unstable();
    vertices.dedup();
    Some(Route { vertices, edges, delay })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Net, Search};

    fn grid(w: usize, h: usize, layers: usize, pins: Vec<i32>, nets: Vec<Net>, ld: Vec<u64>, via: u64) -> Instance {
        let wh = w * h;
        let size = wh * layers;
        let mut neighbors = vec![Vec::new(); size];
        for u in 0..size {
            let z = u / wh; let r = u % wh; let y = r / w; let x = r % w;
            if x + 1 < w { neighbors[u].push((u + 1, ld[z])); }
            if x > 0 { neighbors[u].push((u - 1, ld[z])); }
            if y + 1 < h { neighbors[u].push((u + w, ld[z])); }
            if y > 0 { neighbors[u].push((u - w, ld[z])); }
            if z + 1 < layers { neighbors[u].push((u + wh, via)); }
            if z > 0 { neighbors[u].push((u - wh, via)); }
        }
        Instance { flow_pricing: 0, construction_weight: 1, fanout_priority: 0, slow_congestion: false,
            legalize_conflicts: 0, cost_distance: false, cost_distance_rooted: false, cost_distance_discount: false,
            name: "authored-marginal".into(), w, h, wh, size, ld, via, nets, cross_profiles: Vec::new(), pins,
            neighbors, free_distance: Vec::new() }
    }

    #[test]
    fn farther_path_collects_near_terminal_and_duplicates_keep_multiplicity() {
        // The farther sink 2 is processed first. Its attachment crosses the
        // two pending occurrences of nearer sink 1; those are not charged in
        // that query, but final reconstruction counts each occurrence. Their
        // aggregate rooted sink-delay is 2 + 2*4 = 10.
        let root = 0; let a = 1; let sink = 2;
        let inst = grid(3, 1, 1, vec![0, 0, 0], vec![Net { id: 0, driver: root, sinks: vec![a, sink, sink], cross_singleton: false }], vec![2], 1);
        let mut search = Search::new(inst.size);
        let mut budget = 10_000;
        let route = route(&inst, &mut search, 0, &[0; 3], &mut budget).unwrap();
        assert_eq!(route.delay, 2 + 2 * 4);
        assert_eq!(route.edges.len(), 2);
        assert_eq!(objective(&inst, 0, &route, &[11, 7, 13]), Some((10u128 * 1024) + 31));
    }

    #[test]
    fn foreign_pins_are_excluded_and_result_is_a_tree() {
        let inst = grid(3, 2, 1,
            vec![0, 1, 1, 0, -1, 0],
            vec![Net { id: 0, driver: 0, sinks: vec![5, 3], cross_singleton: false },
                 Net { id: 1, driver: 2, sinks: vec![1], cross_singleton: false }],
            vec![1], 1);
        let mut search = Search::new(inst.size); let mut budget = 10_000;
        let r = route(&inst, &mut search, 0, &[0; 6], &mut budget).unwrap();
        assert!(!r.vertices.contains(&1) && !r.vertices.contains(&2));
        assert!(r.vertices.contains(&3) && r.vertices.contains(&5));
        let mut seen = vec![0usize; inst.size];
        for &v in &r.vertices { seen[v] += 1; }
        assert!(seen.iter().all(|&n| n <= 1));
        assert_eq!(r.edges.len() + 1, r.vertices.len());
        assert!(inst.nets[0].sinks.iter().all(|s| r.vertices.contains(s)));
        assert!(r.edges.iter().all(|&(a, b)| r.vertices.contains(&a) && r.vertices.contains(&b)));
    }

    #[test]
    fn cap_refuses_partial_tree_and_large_tolls_use_wide_keys() {
        let inst = grid(3, 1, 1, vec![0, -1, 0], vec![Net { id: 0, driver: 0, sinks: vec![2], cross_singleton: false }], vec![1], 1);
        let mut search = Search::new(inst.size); let mut budget = 1;
        assert!(route(&inst, &mut search, 0, &[u64::MAX; 3], &mut budget).is_none());
        let mut budget = 10_000;
        assert!(route(&inst, &mut search, 0, &[u64::MAX; 3], &mut budget).is_some());
    }
}
