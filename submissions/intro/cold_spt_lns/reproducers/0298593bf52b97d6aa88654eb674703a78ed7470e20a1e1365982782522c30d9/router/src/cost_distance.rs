//! Experimental cost-distance component merging, inspired by Held/Perner 2025.
//! Costs pay congestion once along a connection; delays carry sink multiplicity.
//! Final union cycles are removed with a physical root-distance tree. This
//! vertex-resource adaptation makes no claim to inherit the paper's guarantee.
use super::*;

#[derive(Clone)]
struct Connection {
    cost: u64,
    target: usize,
    path: Vec<usize>,
}
struct Component {
    anchor: usize,
    weight: u64,
    nearest: Option<Connection>,
}

fn nearest(
    inst: &Instance,
    search: &mut Search,
    nid: usize,
    source: usize,
    goals: &[usize],
    weight: u64,
    count: &[u32],
    history: &[u64],
    present: u64,
    network_epoch: u64,
    salt: u64,
) -> Option<Connection> {
    search.epoch += 1;
    search.h_epoch += 1;
    let epoch = search.epoch;
    search.queue.clear();
    search.stamp[source] = epoch;
    search.dist[source] = 0;
    search.prev[source] = source;
    let first = search.heuristic(inst, source, goals) * weight * 1024;
    search.queue.push(Reverse((
        first,
        u64::MAX,
        mix(source as u64 ^ salt),
        source,
        0,
    )));
    while let Some(Reverse((_, _, _, u, distance))) = search.queue.pop() {
        if search.stamp[u] != epoch || search.dist[u] != distance || search.done[u] == epoch {
            continue;
        }
        search.done[u] = epoch;
        if goals.contains(&u) {
            let mut path = vec![u];
            let mut v = u;
            while v != source {
                v = search.prev[v];
                path.push(v);
            }
            path.reverse();
            return Some(Connection {
                cost: distance,
                target: u,
                path,
            });
        }
        for &(v, delay) in &inst.neighbors[u] {
            if inst.pins[v] >= 0 && inst.pins[v] != nid as i32 {
                continue;
            }
            let congestion = if inst.cost_distance_discount && search.tree_stamp[v] == network_epoch
            {
                0
            } else {
                history[v] + present * count[v] as u64
            };
            let next = distance + delay * weight * 1024 + congestion;
            if search.done[v] != epoch && (search.stamp[v] != epoch || next < search.dist[v]) {
                search.stamp[v] = epoch;
                search.dist[v] = next;
                search.prev[v] = u;
                let f = next + search.heuristic(inst, v, goals) * weight * 1024;
                search
                    .queue
                    .push(Reverse((f, u64::MAX - next, mix(v as u64 ^ salt), v, next)));
            }
        }
    }
    None
}

pub(super) fn route(
    inst: &Instance,
    search: &mut Search,
    nid: usize,
    count: &[u32],
    history: &[u64],
    present: u64,
    salt: u64,
) -> Option<Route> {
    let net = &inst.nets[nid];
    search.tree_epoch += 1;
    let network_epoch = search.tree_epoch;
    search.tree_stamp[net.driver] = network_epoch;
    let mut components: Vec<_> = net
        .sinks
        .iter()
        .map(|&anchor| Component {
            anchor,
            weight: 1,
            nearest: None,
        })
        .collect();
    let mut union = Vec::new();
    let mut iteration = 0;
    while !components.is_empty() {
        let mut active: Vec<_> = components.iter().map(|c| c.anchor).collect();
        active.push(net.driver);
        // Cached paths remain valid until a target is removed. A new embedded
        // anchor or newly discounted resources invalidate affected caches below.
        for component in &mut components {
            if component
                .nearest
                .as_ref()
                .map_or(true, |c| !active.contains(&c.target))
            {
                let goals: Vec<_> = active
                    .iter()
                    .copied()
                    .filter(|&v| v != component.anchor)
                    .collect();
                component.nearest = nearest(
                    inst,
                    search,
                    nid,
                    component.anchor,
                    &goals,
                    component.weight,
                    count,
                    history,
                    present,
                    network_epoch,
                    salt ^ iteration,
                );
            }
        }
        let i = (0..components.len())
            .filter(|&i| components[i].nearest.is_some())
            .min_by_key(|&i| {
                (
                    components[i].nearest.as_ref().unwrap().cost,
                    mix(components[i].anchor as u64 ^ salt ^ iteration),
                )
            })?;
        let chosen = components.remove(i);
        let connection = chosen.nearest.unwrap();
        for &v in &connection.path {
            search.tree_stamp[v] = network_epoch;
        }
        for pair in connection.path.windows(2) {
            union.push((pair[0].min(pair[1]), pair[0].max(pair[1])));
        }
        if connection.target != net.driver {
            let j = components
                .iter()
                .position(|c| c.anchor == connection.target)
                .unwrap();
            let other = components.remove(j);
            let weight = chosen.weight + other.weight;
            let anchor = if inst.cost_distance_rooted {
                // Estimate root reconnection plus the two branches' weighted
                // physical delays. All branching penalties are zero here.
                let total: u64 = connection
                    .path
                    .windows(2)
                    .map(|p| inst.weight(p[0], p[1]))
                    .sum();
                let mut prefix = 0;
                let mut candidates = Vec::new();
                for (position, &v) in connection.path.iter().enumerate() {
                    if position > 0 {
                        prefix += inst.weight(connection.path[position - 1], v);
                    }
                    let estimate = chosen.weight * prefix
                        + other.weight * (total - prefix)
                        + weight * inst.lower_bound(v, &[net.driver]);
                    candidates.push((estimate, mix(v as u64 ^ salt ^ iteration), v));
                }
                candidates.into_iter().min().unwrap().2
            } else if mix(salt ^ iteration ^ chosen.anchor as u64) % weight < chosen.weight {
                chosen.anchor
            } else {
                other.anchor
            };
            if inst.cost_distance_rooted {
                for component in &mut components {
                    if component.nearest.as_ref().map_or(false, |c| {
                        component.weight * 1024 * inst.lower_bound(component.anchor, &[anchor])
                            < c.cost
                    }) {
                        component.nearest = None;
                    }
                }
            }
            components.push(Component {
                anchor,
                weight,
                nearest: None,
            });
        }
        if inst.cost_distance_discount {
            // A reused vertex is paid for once by this candidate tree. Its
            // discounted price can improve any cached path, so recompute all.
            for component in &mut components {
                component.nearest = None;
            }
        }
        iteration += 1;
    }
    union.sort_unstable();
    union.dedup();
    // Embedded connections can overlap. Extract an acyclic physical SPT from
    // their union, then prune every branch that reaches no original sink.
    let mut adj: HashMap<usize, Vec<(usize, u64)>> = HashMap::new();
    for &(a, b) in &union {
        let w = inst.weight(a, b);
        adj.entry(a).or_default().push((b, w));
        adj.entry(b).or_default().push((a, w));
    }
    let mut dist = HashMap::from([(net.driver, 0u64)]);
    let mut prev = HashMap::new();
    let mut queue = BinaryHeap::from([Reverse((0u64, net.driver))]);
    while let Some(Reverse((d, u))) = queue.pop() {
        if dist.get(&u) != Some(&d) {
            continue;
        }
        if let Some(neighbors) = adj.get(&u) {
            for &(v, w) in neighbors {
                let next = d + w;
                if dist.get(&v).map_or(true, |&old| next < old) {
                    dist.insert(v, next);
                    prev.insert(v, u);
                    queue.push(Reverse((next, v)));
                }
            }
        }
    }
    let mut edges = Vec::new();
    let mut delay = 0;
    for &sink in &net.sinks {
        delay += dist.get(&sink)?;
        let mut u = sink;
        while u != net.driver {
            let v = *prev.get(&u)?;
            edges.push((u.min(v), u.max(v)));
            u = v;
        }
    }
    edges.sort_unstable();
    edges.dedup();
    let mut vertices = vec![net.driver];
    for &(a, b) in &edges {
        vertices.push(a);
        vertices.push(b);
    }
    vertices.sort_unstable();
    vertices.dedup();
    Some(Route {
        vertices,
        edges,
        delay,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weighted_nearest_matches_full_dijkstra() {
        let mut rng = Rng(77241);
        for _ in 0..60 {
            let mut inst = Instance {
                flow_pricing: 0,
                construction_weight: 1,
                fanout_priority: 0,
                slow_congestion: false,
                legalize_conflicts: 0,
                cost_distance: false,
                cost_distance_rooted: false,
                cost_distance_discount: false,
                name: "cost-distance".into(),
                w: 3,
                h: 3,
                wh: 9,
                size: 18,
                ld: vec![1, 3],
                via: 2,
                nets: vec![Net {
                    id: 0,
                    driver: 0,
                    sinks: vec![8, 17],
                    cross_singleton: false,
                }],
                pins: vec![-1; 18],
                cross_profiles: vec![None; 1],
                neighbors: vec![vec![]; 18],
                free_distance: Vec::new(),
            };
            inst.pins[0] = 0;
            inst.pins[8] = 0;
            inst.pins[17] = 0;
            for u in 0..18 {
                for v in 0..18 {
                    let a = inst.coord(u);
                    let b = inst.coord(v);
                    if a[0].abs_diff(b[0]) + a[1].abs_diff(b[1]) + a[2].abs_diff(b[2]) == 1 {
                        let w = inst.weight(u, v);
                        inst.neighbors[u].push((v, w));
                    }
                }
            }
            if rng.next() % 2 == 0 {
                inst.pins[4] = 1;
            }
            let count: Vec<u32> = (0..18).map(|_| (rng.next() % 3) as u32).collect();
            let hist: Vec<u64> = (0..18).map(|_| rng.next() % 9000).collect();
            let weight = 1 + rng.next() % 8;
            inst.cost_distance_discount = rng.next() % 2 == 0;
            let discounted: Vec<_> = (0..18).map(|_| rng.next() % 2 == 0).collect();
            let mut search = Search::new(18);
            for v in 0..18 {
                if discounted[v] {
                    search.tree_stamp[v] = 1;
                }
            }
            let mut d = vec![u64::MAX; 18];
            let mut done = vec![false; 18];
            d[0] = 0;
            for _ in 0..18 {
                let u = (0..18).filter(|&u| !done[u]).min_by_key(|&u| d[u]).unwrap();
                if d[u] == u64::MAX {
                    break;
                }
                done[u] = true;
                for &(v, w) in &inst.neighbors[u] {
                    if inst.pins[v] > 0 {
                        continue;
                    }
                    let price = if inst.cost_distance_discount && discounted[v] {
                        0
                    } else {
                        hist[v] + count[v] as u64 * 2048
                    };
                    d[v] = d[v].min(d[u] + w * weight * 1024 + price);
                }
            }
            let result = nearest(
                &inst,
                &mut search,
                0,
                0,
                &[8, 17],
                weight,
                &count,
                &hist,
                2048,
                1,
                rng.next(),
            )
            .unwrap();
            assert_eq!(result.cost, d[8].min(d[17]));
            assert_eq!(result.path[0], 0);
            assert!(result.target == 8 || result.target == 17);
            assert!(result.path.iter().all(|&v| inst.pins[v] <= 0));
        }
    }
}
