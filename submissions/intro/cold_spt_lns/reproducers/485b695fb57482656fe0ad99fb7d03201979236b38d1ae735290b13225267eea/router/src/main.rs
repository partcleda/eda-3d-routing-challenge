//! Independent cold construction from instance pins, followed by fixed-work LNS.
//! No route input, reference solution, learned weights, or embedded route data.
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::fs;
mod cost_distance;
#[allow(dead_code)]
mod refine;
mod subset_tree;

#[derive(Clone)]
struct Net {
    id: u64,
    driver: usize,
    sinks: Vec<usize>,
    cross_singleton: bool,
}
struct Instance {
    flow_pricing: u64,
    construction_weight: u64,
    fanout_priority: u64,
    slow_congestion: bool,
    legalize_conflicts: usize,
    cost_distance: bool,
    cost_distance_rooted: bool,
    cost_distance_discount: bool,
    name: String,
    w: usize,
    h: usize,
    wh: usize,
    size: usize,
    ld: Vec<u64>,
    via: u64,
    nets: Vec<Net>,
    // Optional raw-input-only profile used as a soft hint during construction.
    cross_profiles: Vec<Option<Vec<usize>>>,
    pins: Vec<i32>,
    neighbors: Vec<Vec<(usize, u64)>>,
    free_distance: Vec<u64>,
}
impl Instance {
    fn load(path: &str) -> Self {
        let v: Value =
            serde_json::from_slice(&fs::read(path).expect("read instance")).expect("instance JSON");
        assert_eq!(v["format"], "m3d-instance");
        let w = v["grid"]["width"].as_u64().unwrap() as usize;
        let h = v["grid"]["height"].as_u64().unwrap() as usize;
        let l = v["grid"]["layers"].as_u64().unwrap() as usize;
        let wh = w * h;
        let size = wh * l;
        let ld: Vec<u64> = v["delay"]["layer_delay"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap())
            .collect();
        let via = v["delay"]["via_delay"].as_u64().unwrap();
        assert!(ld.iter().all(|&x| x > 0) && via > 0);
        let mut ids = HashMap::new();
        let mut pin_dies = HashMap::new();
        for p in v["pins"].as_array().unwrap() {
            pin_dies.insert(p["id"].as_u64().unwrap(), p["die"].as_u64().unwrap());
            ids.insert(
                p["id"].as_u64().unwrap(),
                (p["z"].as_u64().unwrap() as usize * h + p["y"].as_u64().unwrap() as usize) * w
                    + p["x"].as_u64().unwrap() as usize,
            );
        }
        let nets: Vec<Net> = v["nets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                let driver_id = n["driver"].as_u64().unwrap();
                let sink_ids: Vec<usize> = n["sinks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| ids[&s.as_u64().unwrap()])
                    .collect();
                let sink_pin_ids: Vec<u64> = n["sinks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s.as_u64().unwrap())
                    .collect();
                Net {
                    id: n["id"].as_u64().unwrap(),
                    driver: ids[&driver_id],
                    sinks: sink_ids,
                    cross_singleton: sink_pin_ids.len() == 1
                        && pin_dies[&driver_id] != pin_dies[&sink_pin_ids[0]],
                }
            })
            .collect();
        let mut pins = vec![-1; size];
        for (i, n) in nets.iter().enumerate() {
            for &p in std::iter::once(&n.driver).chain(n.sinks.iter()) {
                assert!(pins[p] < 0);
                pins[p] = i as i32;
            }
        }
        let mut neighbors = vec![Vec::new(); size];
        for u in 0..size {
            let (z, r) = (u / wh, u % wh);
            let (y, x) = (r / w, r % w);
            if x + 1 < w {
                neighbors[u].push((u + 1, ld[z]));
            }
            if x > 0 {
                neighbors[u].push((u - 1, ld[z]));
            }
            if y + 1 < h {
                neighbors[u].push((u + w, ld[z]));
            }
            if y > 0 {
                neighbors[u].push((u - w, ld[z]));
            }
            if z + 1 < l {
                neighbors[u].push((u + wh, via));
            }
            if z > 0 {
                neighbors[u].push((u - wh, via));
            }
        }
        let mut free_distance = Vec::new();
        for a in 0..l {
            for b in 0..l {
                for xy in 0..w + h - 1 {
                    free_distance.push(
                        ld.iter()
                            .enumerate()
                            .map(|(z, &cost)| {
                                xy as u64 * cost + (a.abs_diff(z) + b.abs_diff(z)) as u64 * via
                            })
                            .min()
                            .unwrap(),
                    );
                }
            }
        }
        Self {
            flow_pricing: 0,
            construction_weight: 1,
            fanout_priority: 0,
            slow_congestion: false,
            legalize_conflicts: 0,
            cost_distance: false,
            cost_distance_rooted: false,
            cost_distance_discount: false,
            name: v["name"].as_str().unwrap().into(),
            w,
            h,
            wh,
            size,
            ld,
            via,
            cross_profiles: vec![None; nets.len()],
            nets,
            pins,
            neighbors,
            free_distance,
        }
    }
    fn coord(&self, u: usize) -> [usize; 3] {
        [u % self.w, u / self.w % self.h, u / self.wh]
    }
    fn weight(&self, a: usize, b: usize) -> u64 {
        if a / self.wh == b / self.wh {
            self.ld[a / self.wh]
        } else {
            self.via
        }
    }
    fn lower_bound(&self, u: usize, goals: &[usize]) -> u64 {
        let a = self.coord(u);
        goals
            .iter()
            .map(|&s| {
                let b = self.coord(s);
                let xy = a[0].abs_diff(b[0]) + a[1].abs_diff(b[1]);
                if !self.free_distance.is_empty() {
                    return self.free_distance
                        [(a[2] * self.ld.len() + b[2]) * (self.w + self.h - 1) + xy];
                }
                self.ld
                    .iter()
                    .enumerate()
                    .map(|(z, &w)| {
                        xy as u64 * w + (a[2].abs_diff(z) + b[2].abs_diff(z)) as u64 * self.via
                    })
                    .min()
                    .unwrap()
            })
            .min()
            .unwrap_or(0)
    }
    // Resource demand is paid once by a tree, while delay is paid by every
    // sink. Normalize congestion prices to favor delay-sensitive fanouts.
    fn congestion_price(&self, nid: usize, price: u64) -> u64 {
        let fanout = self.nets[nid].sinks.len().max(1) as u64;
        let divisor = match self.fanout_priority {
            0 => 1024,
            1 => (fanout * 1024 * 1024).isqrt(),
            2 => fanout * 1024,
            _ => panic!("invalid fanout priority"),
        };
        price * 1024 / divisor
    }
}
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        mix(self.0)
    }
    fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.next() as usize % (i + 1);
            v.swap(i, j);
        }
    }
}
#[derive(Clone, Debug)]
struct Route {
    vertices: Vec<usize>,
    edges: Vec<(usize, usize)>,
    delay: u64,
}
struct Search {
    layer_direction_preference: u64,
    layer_direction_phase: u64,
    direction_phase_stats: DirectionPhaseStats,
    flow: Vec<u64>,
    flow_stamp: Vec<u64>,
    flow_epoch: u64,
    tree_stamp: Vec<u64>,
    tree_price: Vec<u64>,
    tree_delay: Vec<u64>,
    tree_epoch: u64,
    dist: Vec<u64>,
    prev: Vec<usize>,
    stamp: Vec<u64>,
    done: Vec<u64>,
    h_stamp: Vec<u64>,
    h_value: Vec<u64>,
    h_epoch: u64,
    epoch: u64,
    queue: BinaryHeap<Reverse<(u64, u64, u64, usize, u64)>>,
}
#[derive(Clone, Debug, Default)]
struct DirectionPhaseStats {
    paired_calls: u64,
    phase0_selected: u64,
    phase1_selected: u64,
    phase0_failed: u64,
    phase1_failed: u64,
    both_failed: u64,
    comparable_calls: u64,
    common_delta_phase1_minus_phase0: i128,
}
impl Search {
    fn new(size: usize) -> Self {
        Self {
            layer_direction_preference: 0,
            layer_direction_phase: 0,
            direction_phase_stats: DirectionPhaseStats::default(),
            flow: vec![0; size],
            flow_stamp: vec![0; size],
            flow_epoch: 0,
            tree_stamp: vec![0; size],
            tree_price: vec![0; size],
            tree_delay: vec![0; size],
            tree_epoch: 0,
            dist: vec![0; size],
            prev: vec![0; size],
            stamp: vec![0; size],
            done: vec![0; size],
            h_stamp: vec![0; size],
            h_value: vec![0; size],
            h_epoch: 0,
            epoch: 0,
            queue: BinaryHeap::new(),
        }
    }
    // Reweight only the previous tree generated within this cold invocation.
    // A shared resource is occupied once even when many sink paths use it.
    fn prepare_flow(&mut self, net: &Net, old: Option<&Route>) {
        self.flow_epoch += 1;
        let Some(old) = old else { return };
        let mut adjacency: HashMap<usize, Vec<usize>> = HashMap::new();
        for &(a, b) in &old.edges {
            adjacency.entry(a).or_default().push(b);
            adjacency.entry(b).or_default().push(a);
        }
        let mut parent = HashMap::new();
        parent.insert(net.driver, net.driver);
        let mut order = vec![net.driver];
        let mut cursor = 0;
        while cursor < order.len() {
            let u = order[cursor];
            cursor += 1;
            self.flow_stamp[u] = self.flow_epoch;
            self.flow[u] = 0;
            if let Some(neighbors) = adjacency.get(&u) {
                for &v in neighbors {
                    if let std::collections::hash_map::Entry::Vacant(e) = parent.entry(v) {
                        e.insert(u);
                        order.push(v);
                    }
                }
            }
        }
        for &sink in &net.sinks {
            assert_eq!(self.flow_stamp[sink], self.flow_epoch);
            self.flow[sink] += 1;
        }
        for &u in order.iter().rev() {
            if u != net.driver {
                self.flow[parent[&u]] += self.flow[u];
            }
        }
    }
    fn flow_price(&self, mode: u64, v: usize, price: u64) -> u64 {
        let flow = if self.flow_stamp[v] == self.flow_epoch {
            self.flow[v].max(1)
        } else {
            1
        };
        match mode {
            1 => price / flow,
            2 => price * 1024 / (flow * 1024 * 1024).isqrt(),
            _ => panic!("invalid flow pricing"),
        }
    }
    fn heuristic(&mut self, inst: &Instance, u: usize, goals: &[usize]) -> u64 {
        if self.h_stamp[u] != self.h_epoch {
            self.h_stamp[u] = self.h_epoch;
            self.h_value[u] = inst.lower_bound(u, goals);
        }
        self.h_value[u]
    }
    // Attach sinks to the existing tree. Root-price weighting interpolates
    // between compact Steiner-like growth (0) and root-distance growth (100).
    // Each new path enters the old tree exactly once, so cycles cannot form.
    fn route_tree(
        &mut self,
        inst: &Instance,
        nid: usize,
        count: &[u32],
        history: &[u64],
        present: u64,
        salt: u64,
        root_weight: u64,
    ) -> Option<Route> {
        let net = &inst.nets[nid];
        let root = net.driver;
        self.tree_epoch += 1;
        let tree_epoch = self.tree_epoch;
        self.tree_stamp[root] = tree_epoch;
        self.tree_price[root] = 0;
        self.tree_delay[root] = 0;
        let mut vertices = vec![root];
        let mut edges = Vec::new();
        let mut sinks = net.sinks.clone();
        sinks.sort_by_key(|&s| (Reverse(inst.lower_bound(root, &[s])), mix(s as u64 ^ salt)));
        for sink in sinks {
            if self.tree_stamp[sink] == tree_epoch {
                continue;
            }
            self.epoch += 1;
            let epoch = self.epoch;
            self.queue.clear();
            for &u in &vertices {
                let d = self.tree_price[u] * root_weight / 100;
                self.stamp[u] = epoch;
                self.dist[u] = d;
                self.prev[u] = u;
                let f = d + inst.lower_bound(u, &[sink]) * 1024 * inst.construction_weight;
                self.queue
                    .push(Reverse((f, u64::MAX - d, mix(u as u64 ^ salt), u, d)));
            }
            let mut reached = false;
            while let Some(Reverse((_, _, _, u, d))) = self.queue.pop() {
                if self.stamp[u] != epoch || self.dist[u] != d || self.done[u] == epoch {
                    continue;
                }
                self.done[u] = epoch;
                if u == sink {
                    reached = true;
                    break;
                }
                for &(v, w) in &inst.neighbors[u] {
                    if self.tree_stamp[v] == tree_epoch
                        || (inst.pins[v] >= 0 && inst.pins[v] != nid as i32)
                    {
                        continue;
                    }
                    let nd = d
                        + w * 1024
                        + inst.congestion_price(nid, history[v] + present * count[v] as u64);
                    if self.done[v] != epoch && (self.stamp[v] != epoch || nd < self.dist[v]) {
                        self.stamp[v] = epoch;
                        self.dist[v] = nd;
                        self.prev[v] = u;
                        let f = nd + inst.lower_bound(v, &[sink]) * 1024 * inst.construction_weight;
                        self.queue
                            .push(Reverse((f, u64::MAX - nd, mix(v as u64 ^ salt), v, nd)));
                    }
                }
            }
            if !reached {
                return None;
            }
            let mut path = Vec::new();
            let mut u = sink;
            while self.tree_stamp[u] != tree_epoch {
                path.push(u);
                u = self.prev[u];
            }
            for v in path.into_iter().rev() {
                let w = inst.weight(u, v);
                self.tree_stamp[v] = tree_epoch;
                self.tree_price[v] = self.tree_price[u]
                    + w * 1024
                    + inst.congestion_price(nid, history[v] + present * count[v] as u64);
                self.tree_delay[v] = self.tree_delay[u] + w;
                edges.push((u.min(v), u.max(v)));
                vertices.push(v);
                u = v;
            }
        }
        let delay = net.sinks.iter().map(|&s| self.tree_delay[s]).sum();
        edges.sort_unstable();
        vertices.sort_unstable();
        Some(Route {
            vertices,
            edges,
            delay,
        })
    }
    // Root-distance SPT: minimizing every driver's sink distance also minimizes
    // their sum for one net with the other nets fixed. Positive costs give a tree.
    fn route(
        &mut self,
        inst: &Instance,
        nid: usize,
        count: &[u32],
        history: &[u64],
        present: u64,
        strict: bool,
        salt: u64,
    ) -> Option<Route> {
        self.epoch += 1;
        self.h_epoch += 1;
        let epoch = self.epoch;
        self.queue.clear();
        let n = &inst.nets[nid];
        let root = n.driver;
        if count[root] > 0 {
            return None;
        }
        self.dist[root] = 0;
        self.prev[root] = root;
        self.stamp[root] = epoch;
        let mut goals = n.sinks.clone();
        let scale = if strict {
            1
        } else {
            1024 * inst.construction_weight
        };
        let first = self.heuristic(inst, root, &goals) * scale;
        self.queue
            .push(Reverse((first, u64::MAX, mix(root as u64 ^ salt), root, 0)));
        while let Some(Reverse((f, _, _, u, d))) = self.queue.pop() {
            if self.stamp[u] != epoch || self.dist[u] != d || self.done[u] == epoch {
                continue;
            }
            // Removing a reached goal raises the consistent heuristic. Old queue
            // keys stay lower bounds; refresh them lazily before expansion.
            let current = d + self.heuristic(inst, u, &goals) * scale;
            if current > f {
                self.queue
                    .push(Reverse((current, u64::MAX - d, mix(u as u64 ^ salt), u, d)));
                continue;
            }
            self.done[u] = epoch;
            if let Some(i) = goals.iter().position(|&s| s == u) {
                goals.remove(i);
                self.h_epoch += 1;
                if goals.is_empty() {
                    break;
                }
            }
            for &(v, w) in &inst.neighbors[u] {
                if inst.pins[v] >= 0 && inst.pins[v] != nid as i32 {
                    continue;
                }
                if strict && count[v] > 0 {
                    continue;
                }
                let extra = if strict {
                    0
                } else {
                    let price = history[v] + present * count[v] as u64;
                    let outside_profile = inst.cross_profiles[nid]
                        .as_ref()
                        .is_some_and(|profile| profile.binary_search(&v).is_err());
                    if inst.flow_pricing > 0 {
                        self.flow_price(inst.flow_pricing, v, price)
                            + if outside_profile { 256 } else { 0 }
                    } else {
                        inst.congestion_price(nid, price) + if outside_profile { 256 } else { 0 }
                    }
                };
                let step = if strict {
                    w
                } else {
                    let direction_mode = if self.layer_direction_preference == 2 {
                        1 + self.layer_direction_phase
                    } else {
                        self.layer_direction_preference
                    };
                    let direction = match lateral_direction_penalty(inst, u, v, direction_mode) {
                        Some(direction) => direction,
                        None if self.layer_direction_preference == 2 => continue,
                        None => panic!("layer direction penalty overflow"),
                    };
                    construction_search_step(w, extra, direction)
                        .expect("construction search step overflow")
                };
                let nd = d
                    .checked_add(step)
                    .expect("construction search cost overflow");
                if self.done[v] != epoch && (self.stamp[v] != epoch || nd < self.dist[v]) {
                    self.stamp[v] = epoch;
                    self.dist[v] = nd;
                    self.prev[v] = u;
                    let priority = nd + self.heuristic(inst, v, &goals) * scale;
                    self.queue.push(Reverse((
                        priority,
                        u64::MAX - nd,
                        mix(v as u64 ^ salt),
                        v,
                        nd,
                    )));
                }
            }
        }
        if !goals.is_empty() {
            return None;
        }
        let mut edges = Vec::new();
        let mut delay = 0;
        for &sink in &n.sinks {
            let mut u = sink;
            while u != root {
                let p = self.prev[u];
                delay += inst.weight(u, p);
                edges.push((p.min(u), p.max(u)));
                u = p;
            }
        }
        edges.sort_unstable();
        edges.dedup();
        let mut vertices = vec![root];
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
}

// Direction mode 1 is phase 0; internal mode 2 is phase 1. Phase 0 prefers X
// on even-numbered layers and Y on odd-numbered layers; phase 1 reverses that.
// The added search cost is soft: no edge is removed from the graph.
fn lateral_direction_penalty(inst: &Instance, u: usize, v: usize, mode: u64) -> Option<u64> {
    if mode == 0 {
        return Some(0);
    }
    if mode != 1 && mode != 2 {
        return None;
    }
    let a = inst.coord(u);
    let b = inst.coord(v);
    if a[2] != b[2] {
        return Some(0);
    }
    let x_move = a[0] != b[0] && a[1] == b[1];
    let y_move = a[1] != b[1] && a[0] == b[0];
    if !x_move && !y_move {
        return Some(0);
    }
    let prefers_x = (a[2] % 2 == 0) ^ (mode == 2);
    let preferred = if prefers_x { x_move } else { y_move };
    if preferred {
        Some(0)
    } else {
        inst.via.checked_mul(1024)
    }
}

fn construction_search_step(physical_weight: u64, congestion: u64, direction: u64) -> Option<u64> {
    physical_weight
        .checked_mul(1024)?
        .checked_add(congestion)?
        .checked_add(direction)
}

// The common comparison removes only the direction-guidance addend. It prices
// sink-delay contribution per sink and congestion once per unique route vertex,
// using this builder's current history/present occupancy and configured fanout
// scaling. It is a conditional candidate score, not an exact multi-net optimum.
fn common_conditional_route_score(
    inst: &Instance,
    nid: usize,
    route: &Route,
    count: &[u32],
    history: &[u64],
    present: u64,
) -> u64 {
    let delay_cost = route
        .delay
        .checked_mul(1024)
        .expect("common conditional delay score overflow");
    route
        .vertices
        .iter()
        .try_fold(delay_cost, |sum, &vertex| {
            let price = history[vertex].checked_add(present.checked_mul(count[vertex] as u64)?)?;
            sum.checked_add(inst.congestion_price(nid, price))
        })
        .expect("common conditional congestion score overflow")
}

fn choose_direction_candidate(
    phase0: Option<(Route, u64)>,
    phase1: Option<(Route, u64)>,
) -> (Option<Route>, Option<u64>, Option<i128>) {
    match (phase0, phase1) {
        (Some((route0, score0)), Some((route1, score1))) => {
            let delta = score1 as i128 - score0 as i128;
            if score1 < score0 {
                (Some(route1), Some(1), Some(delta))
            } else {
                // Equal common scores retain phase 0 for deterministic ties.
                (Some(route0), Some(0), Some(delta))
            }
        }
        (Some((route, _)), None) => (Some(route), Some(0), None),
        (None, Some((route, _))) => (Some(route), Some(1), None),
        (None, None) => (None, None, None),
    }
}

fn paired_direction_route(
    inst: &Instance,
    search: &mut Search,
    nid: usize,
    count: &[u32],
    history: &[u64],
    present: u64,
    salt: u64,
) -> Option<Route> {
    debug_assert_eq!(search.layer_direction_preference, 2);
    let saved_phase = search.layer_direction_phase;
    // Both calls borrow the same immutable occupancy/history, use the same
    // present price and salt, and make no RNG draws or builder-state updates.
    search.layer_direction_phase = 0;
    let phase0 = search.route(inst, nid, count, history, present, false, salt);
    search.layer_direction_phase = 1;
    let phase1 = search.route(inst, nid, count, history, present, false, salt);
    search.layer_direction_phase = saved_phase;

    let score0 = phase0
        .as_ref()
        .map(|route| common_conditional_route_score(inst, nid, route, count, history, present));
    let score1 = phase1
        .as_ref()
        .map(|route| common_conditional_route_score(inst, nid, route, count, history, present));
    let (route, selected_phase, delta) =
        choose_direction_candidate(phase0.zip(score0), phase1.zip(score1));
    let stats = &mut search.direction_phase_stats;
    stats.paired_calls += 1;
    if score0.is_none() {
        stats.phase0_failed += 1;
    }
    if score1.is_none() {
        stats.phase1_failed += 1;
    }
    if score0.is_none() && score1.is_none() {
        stats.both_failed += 1;
    }
    if let Some(phase) = selected_phase {
        if phase == 0 {
            stats.phase0_selected += 1;
        } else {
            stats.phase1_selected += 1;
        }
    }
    if let Some(delta) = delta {
        stats.comparable_calls += 1;
        stats.common_delta_phase1_minus_phase0 = stats
            .common_delta_phase1_minus_phase0
            .checked_add(delta)
            .expect("direction phase objective telemetry overflow");
    }
    eprintln!(
        "direction-phase-choice net={} selected={} phase0_common={:?} phase1_common={:?} phase1_minus_phase0={:?} paired_query=true guidance_active={}",
        inst.nets[nid].id,
        selected_phase.map_or_else(|| "none".to_string(), |phase| phase.to_string()),
        score0,
        score1,
        delta,
        selected_phase.is_some()
    );
    route
}

#[derive(Default)]
struct DirectionTelemetry {
    by_layer: Vec<(u64, u64)>,
    preferred: u64,
    nonpreferred: u64,
    vias: u64,
    central_cut: u64,
}

fn direction_telemetry(inst: &Instance, routes: &[Route]) -> DirectionTelemetry {
    let mut stats = DirectionTelemetry {
        by_layer: vec![(0, 0); inst.ld.len()],
        ..DirectionTelemetry::default()
    };
    for route in routes {
        for &(u, v) in &route.edges {
            let a = inst.coord(u);
            let b = inst.coord(v);
            if a[2] != b[2] {
                stats.vias += 1;
                continue;
            }
            let z = a[2];
            let x_move = a[0] != b[0];
            if x_move {
                stats.by_layer[z].0 += 1;
            } else {
                stats.by_layer[z].1 += 1;
            }
            let preferred = if z % 2 == 0 { x_move } else { !x_move };
            if preferred {
                stats.preferred += 1;
            } else {
                stats.nonpreferred += 1;
            }
            if z + 1 == inst.ld.len() / 2 || z == inst.ld.len() / 2 {
                stats.central_cut += 1;
            }
        }
    }
    stats
}

fn direction_mode_compatible(
    mode: u64,
    cost_distance: bool,
    tree_root_weight: Option<u64>,
    subset_tree_max_sinks: usize,
    cross_profile_plan: u64,
    flow_pricing: u64,
) -> bool {
    mode <= 2
        && (mode == 0
            || (!cost_distance
                && tree_root_weight.is_none()
                && subset_tree_max_sinks == 0
                && cross_profile_plan == 0
                && flow_pricing == 0))
}

fn with_uniform_direction_preference<T>(
    search: &mut Search,
    run: impl FnOnce(&mut Search) -> T,
) -> T {
    let requested = search.layer_direction_preference;
    let requested_phase = search.layer_direction_phase;
    search.layer_direction_preference = 0;
    search.layer_direction_phase = 0;
    let result = run(search);
    search.layer_direction_preference = requested;
    search.layer_direction_phase = requested_phase;
    result
}
fn occupy(count: &mut [u32], r: &Route, add: bool) {
    for &v in &r.vertices {
        if add {
            count[v] += 1;
        } else {
            count[v] -= 1;
        }
    }
}

#[derive(Clone)]
struct CrossProfile {
    plane: usize,
    elbow: usize,
    vertices: Vec<usize>,
    edges: Vec<(usize, usize)>,
    delay: u64,
}

fn profile_for(inst: &Instance, nid: usize, plane: usize, elbow: usize) -> CrossProfile {
    let net = &inst.nets[nid];
    assert!(net.cross_singleton && net.sinks.len() == 1);
    assert!(plane == 2 || plane == 3);
    let (mut x, mut y, z0) = {
        let c = inst.coord(net.driver);
        (c[0], c[1], c[2])
    };
    let sink = net.sinks[0];
    let sc = inst.coord(sink);
    let mut path = vec![net.driver];
    let mut z = z0;
    while z != plane {
        z = if z < plane { z + 1 } else { z - 1 };
        path.push(z * inst.wh + y * inst.w + x);
    }
    if elbow == 0 {
        while x != sc[0] {
            x = if x < sc[0] { x + 1 } else { x - 1 };
            path.push(plane * inst.wh + y * inst.w + x);
        }
        while y != sc[1] {
            y = if y < sc[1] { y + 1 } else { y - 1 };
            path.push(plane * inst.wh + y * inst.w + x);
        }
    } else {
        while y != sc[1] {
            y = if y < sc[1] { y + 1 } else { y - 1 };
            path.push(plane * inst.wh + y * inst.w + x);
        }
        while x != sc[0] {
            x = if x < sc[0] { x + 1 } else { x - 1 };
            path.push(plane * inst.wh + y * inst.w + x);
        }
    }
    z = plane;
    while z != sc[2] {
        z = if z < sc[2] { z + 1 } else { z - 1 };
        path.push(z * inst.wh + sc[1] * inst.w + sc[0]);
    }
    assert_eq!(path.last().copied(), Some(sink));
    let mut edges = Vec::with_capacity(path.len().saturating_sub(1));
    let mut delay = 0;
    for pair in path.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        assert!(inst.neighbors[a].iter().any(|&(v, _)| v == b));
        edges.push((a.min(b), a.max(b)));
        delay += inst.weight(a, b);
    }
    let mut vertices = path;
    vertices.sort_unstable();
    vertices.dedup();
    edges.sort_unstable();
    edges.dedup();
    CrossProfile {
        plane,
        elbow,
        vertices,
        edges,
        delay,
    }
}

fn intersection_count<T: Ord>(a: &[T], b: &[T]) -> usize {
    let (mut i, mut j, mut n) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                n += 1;
                i += 1;
                j += 1;
            }
        }
    }
    n
}

fn profile_overlap(a: &CrossProfile, b: &Route) -> (usize, usize) {
    (
        intersection_count(&a.vertices, &b.vertices),
        intersection_count(&a.edges, &b.edges),
    )
}

fn profile_profile_overlap(a: &CrossProfile, b: &CrossProfile) -> (usize, usize) {
    (
        intersection_count(&a.vertices, &b.vertices),
        intersection_count(&a.edges, &b.edges),
    )
}

fn profile_score(
    target_pos: usize,
    option: usize,
    chosen: &[usize],
    target: &[usize],
    profiles: &[Vec<CrossProfile>],
    outside: &[Route],
) -> u64 {
    let p = &profiles[target_pos][option];
    let mut score = 0u64;
    for route in outside {
        let (v, e) = profile_overlap(p, route);
        score += (v + e) as u64;
    }
    for other_pos in 0..target.len() {
        if other_pos == target_pos {
            continue;
        }
        let (v, e) = profile_profile_overlap(p, &profiles[other_pos][chosen[other_pos]]);
        score += (v + e) as u64;
    }
    score
}

fn profile_metrics(
    chosen: &[usize],
    profiles: &[Vec<CrossProfile>],
    outside: &[Route],
) -> (u64, u64) {
    let mut resource_sum = 0u64;
    let mut overlapping_pairs = 0u64;
    for (i, &option) in chosen.iter().enumerate() {
        let p = &profiles[i][option];
        for r in outside {
            let (v, e) = profile_overlap(p, r);
            resource_sum += (v + e) as u64;
            if v + e > 0 {
                overlapping_pairs += 1;
            }
        }
        for j in i + 1..chosen.len() {
            let (v, e) = profile_profile_overlap(p, &profiles[j][chosen[j]]);
            resource_sum += (v + e) as u64;
            if v + e > 0 {
                overlapping_pairs += 1;
            }
        }
    }
    (resource_sum, overlapping_pairs)
}

fn balanced_initial_profile_choice(target_pos: usize) -> usize {
    if target_pos % 2 == 0 {
        0
    } else {
        2
    }
}

// Plans once from raw pins only. The private planner has its own scratch search
// and fixed salts, so this prepass cannot consume or perturb the construction RNG.
fn plan_cross_profiles(inst: &mut Instance, mode: u64, seed: u64) {
    assert!(mode == 1 || mode == 2);
    let mut target: Vec<usize> = (0..inst.nets.len())
        .filter(|&i| inst.nets[i].cross_singleton)
        .collect();
    target.sort_by_key(|&i| inst.nets[i].id);
    let mut profiles = Vec::with_capacity(target.len());
    let mut invalid_pin_hits = 0usize;
    for &nid in &target {
        let net = &inst.nets[nid];
        let lower = inst.lower_bound(net.driver, &net.sinks);
        let choices: Vec<CrossProfile> = [2usize, 3]
            .into_iter()
            .flat_map(|plane| [0usize, 1].map(move |elbow| (plane, elbow)))
            .map(|(plane, elbow)| profile_for(inst, nid, plane, elbow))
            .collect();
        for p in &choices {
            assert_eq!(
                p.delay, lower,
                "cross profile must attain exact physical LB"
            );
            assert!(p.vertices.binary_search(&net.driver).is_ok());
            assert!(p.vertices.binary_search(&net.sinks[0]).is_ok());
            assert!(p.edges.len() + 1 == p.vertices.len());
            invalid_pin_hits += p
                .vertices
                .iter()
                .filter(|&&v| inst.pins[v] >= 0 && inst.pins[v] != nid as i32)
                .count();
        }
        profiles.push(choices);
    }
    assert_eq!(invalid_pin_hits, 0, "raw profile intersects a foreign pin");

    let zero = vec![0u32; inst.size];
    let history = vec![0u64; inst.size];
    let mut planner = Search::new(inst.size);
    let mut outside = Vec::with_capacity(inst.nets.len() - target.len());
    for nid in 0..inst.nets.len() {
        if inst.nets[nid].cross_singleton {
            continue;
        }
        let salt = mix(seed ^ inst.nets[nid].id ^ 0x6372_6f73_735f_7370);
        let route = planner
            .route(inst, nid, &zero, &history, 0, true, salt)
            .expect("raw pin-only outside SPT must connect");
        outside.push(route);
    }

    // Same balanced initialization in both active modes, sorted by raw net ID.
    let mut chosen: Vec<usize> = (0..target.len())
        .map(balanced_initial_profile_choice)
        .collect();
    let initial = chosen.clone();
    let (initial_score, initial_pairs) = profile_metrics(&chosen, &profiles, &outside);
    let mut sweeps = 0usize;
    for _ in 0..12 {
        sweeps += 1;
        let mut changed = false;
        for pos in 0..target.len() {
            let allowed: Vec<usize> = if mode == 1 {
                let plane_start = balanced_initial_profile_choice(pos);
                vec![plane_start, plane_start + 1]
            } else {
                vec![0, 1, 2, 3]
            };
            let current = chosen[pos];
            let mut best_score = u64::MAX;
            let mut tied = Vec::new();
            for option in allowed {
                let score = profile_score(pos, option, &chosen, &target, &profiles, &outside);
                if score < best_score {
                    best_score = score;
                    tied.clear();
                    tied.push(option);
                } else if score == best_score {
                    tied.push(option);
                }
            }
            let best = if tied.contains(&current) {
                current
            } else {
                *tied.iter().min().unwrap()
            };
            if best != current {
                chosen[pos] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let (final_score, final_pairs) = profile_metrics(&chosen, &profiles, &outside);
    let mut plane2 = 0usize;
    let mut plane3 = 0usize;
    let mut elbow_xy = 0usize;
    let mut elbow_yx = 0usize;
    for (pos, &nid) in target.iter().enumerate() {
        let p = &profiles[pos][if mode == 1 {
            let fixed_plane_start = if pos % 2 == 0 { 0 } else { 2 };
            let elbow = chosen[pos] % 2;
            fixed_plane_start + elbow
        } else {
            chosen[pos]
        }];
        assert!(p.delay == inst.lower_bound(inst.nets[nid].driver, &inst.nets[nid].sinks));
        if p.plane == 2 {
            plane2 += 1
        } else {
            plane3 += 1
        }
        if p.elbow == 0 {
            elbow_xy += 1
        } else {
            elbow_yx += 1
        }
        inst.cross_profiles[nid] = Some(p.vertices.clone());
    }
    let (initial2, initial3) = initial.iter().fold((0usize, 0usize), |(a, b), &choice| {
        if choice < 2 {
            (a + 1, b)
        } else {
            (a, b + 1)
        }
    });
    eprintln!(
        "cross-profile plan mode={} targets={} outside_raw_spts={} invalid_foreign_pins={} init_planes={}/{} init_overlap={}/{} selected_planes={}/{} elbows_xy/yx={}/{} overlap={}/{} sweeps={} lb_templates={}",
        mode,
        target.len(),
        outside.len(),
        invalid_pin_hits,
        initial2,
        initial3,
        initial_score,
        initial_pairs,
        plane2,
        plane3,
        elbow_xy,
        elbow_yx,
        final_score,
        final_pairs,
        sweeps,
        target.len() * 4,
    );
}

fn report_profile_adherence(inst: &Instance, routes: &[Route]) {
    let mut selected_vertices = 0usize;
    let mut on_profile_vertices = 0usize;
    let mut full_profiles = 0usize;
    let mut profiled_nets = 0usize;
    for (nid, route) in routes.iter().enumerate() {
        let Some(profile) = &inst.cross_profiles[nid] else {
            continue;
        };
        profiled_nets += 1;
        selected_vertices += profile.len();
        let overlap = intersection_count(profile, &route.vertices);
        on_profile_vertices += overlap;
        if overlap == profile.len() {
            full_profiles += 1;
        }
    }
    eprintln!(
        "cross-profile construction adherence nets={} full_profiles={} on_profile_vertices={}/{}",
        profiled_nets, full_profiles, on_profile_vertices, selected_vertices
    );
}

// Remove all conflicting trees, then reconnect them against fixed legal nets.
// A failed attempt restores the exact incumbent occupancy and geometry.
fn legalize_conflicts(
    inst: &Instance,
    search: &mut Search,
    routes: &mut [Option<Route>],
    count: &mut [u32],
    rng: &mut Rng,
) -> bool {
    let mut ids: Vec<_> = routes
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            r.as_ref()
                .filter(|r| r.vertices.iter().any(|&v| count[v] > 1))
                .map(|_| i)
        })
        .collect();
    for &i in &ids {
        occupy(count, routes[i].as_ref().unwrap(), false);
    }
    rng.shuffle(&mut ids);
    let history = vec![0; inst.size];
    let mut placed = Vec::new();
    for &i in &ids {
        if let Some(route) = search.route(inst, i, count, &history, 0, true, rng.next()) {
            occupy(count, &route, true);
            placed.push((i, route));
        } else {
            for (_, route) in &placed {
                occupy(count, route, false);
            }
            for &j in &ids {
                occupy(count, routes[j].as_ref().unwrap(), true);
            }
            return false;
        }
    }
    for (i, route) in placed {
        routes[i] = Some(route);
    }
    assert!(count.iter().all(|&n| n <= 1));
    true
}
fn build(
    inst: &Instance,
    search: &mut Search,
    rng: &mut Rng,
    limit: usize,
    tree_root_weight: Option<u64>,
    negotiation_rounds: usize,
    subset_tree_max_sinks: usize,
) -> Option<Vec<Route>> {
    let k = inst.nets.len();
    let mut count = vec![0; inst.size];
    let mut history = vec![0; inst.size];
    let mut routes: Vec<Option<Route>> = vec![None; k];
    let mut order: Vec<_> = (0..k).collect();
    rng.shuffle(&mut order);
    let initial_present = if inst.slow_congestion { 154 } else { 2048 };
    let history_increment = if inst.slow_congestion { 154 } else { 4096 };
    let mut present = initial_present;
    let mut best: Option<Vec<Route>> = None;
    let mut best_total = u64::MAX;
    let mut force_all = false;
    let mut remaining = negotiation_rounds;
    let mut subset_attempts = 0usize;
    let mut subset_candidates = 0usize;
    let mut subset_kept_incumbent = 0usize;
    let mut subset_skips = 0usize;
    let mut subset_fallbacks = 0usize;
    for round in 0..limit + negotiation_rounds {
        if best.is_none() && round >= limit {
            break;
        }
        if best.is_some() {
            if remaining == 0 {
                break;
            }
            remaining -= 1;
        }
        let affected: Vec<bool> = routes
            .iter()
            .map(|r| {
                force_all
                    || r.as_ref()
                        .map_or(true, |r| r.vertices.iter().any(|&v| count[v] > 1))
            })
            .collect();
        force_all = false;
        rng.shuffle(&mut order);
        for &i in &order {
            if !affected[i] {
                continue;
            }
            if inst.flow_pricing > 0 {
                search.prepare_flow(&inst.nets[i], routes[i].as_ref());
            }
            let old_route = routes[i].clone();
            if let Some(ref r) = routes[i] {
                occupy(&mut count, r, false);
            }
            let salt = rng.next();
            let subset_eligible = subset_tree_max_sinks > 0
                && (2..=subset_tree_max_sinks).contains(&inst.nets[i].sinks.len());
            if subset_eligible {
                subset_attempts += 1;
            }
            let exact_small_tree = if subset_eligible {
                match subset_tree::route(inst, i, &count, &history, present) {
                    Ok(Some((candidate, candidate_cost))) => {
                        subset_candidates += 1;
                        if let Some(old) = old_route.as_ref() {
                            let old_cost = old
                                .delay
                                .checked_mul(1024)
                                .and_then(|x| {
                                    old.vertices.iter().try_fold(x, |sum, &v| {
                                        sum.checked_add(inst.congestion_price(
                                            i,
                                            history[v].checked_add(
                                                present.checked_mul(count[v] as u64)?,
                                            )?,
                                        ))
                                    })
                                })
                                .expect("old route priced objective overflow");
                            assert!(
                                old_cost >= candidate_cost,
                                "exact subset tree is worse than feasible incumbent"
                            );
                            if old_cost == candidate_cost {
                                subset_kept_incumbent += 1;
                                Some(old.clone())
                            } else {
                                Some(candidate)
                            }
                        } else {
                            Some(candidate)
                        }
                    }
                    Ok(None) => {
                        subset_skips += 1;
                        eprintln!("subset-tree kernel found no route for net {}; using configured route kernel", inst.nets[i].id);
                        None
                    }
                    Err(reason) => {
                        subset_skips += 1;
                        eprintln!(
                            "subset-tree kernel skipped net {}: {}; using configured route kernel",
                            inst.nets[i].id, reason
                        );
                        None
                    }
                }
            } else {
                None
            };
            let r = if let Some(route) = exact_small_tree {
                route
            } else {
                if subset_eligible {
                    subset_fallbacks += 1;
                }
                if inst.cost_distance {
                    cost_distance::route(inst, search, i, &count, &history, present, salt)?
                } else if let Some(weight) = tree_root_weight {
                    search.route_tree(inst, i, &count, &history, present, salt, weight)?
                } else if search.layer_direction_preference == 2 {
                    paired_direction_route(inst, search, i, &count, &history, present, salt)?
                } else {
                    search.route(inst, i, &count, &history, present, false, salt)?
                }
            };
            occupy(&mut count, &r, true);
            routes[i] = Some(r);
        }
        let mut over = 0;
        for v in 0..inst.size {
            if count[v] > 1 {
                over += 1;
                history[v] += history_increment * (count[v] - 1) as u64;
            }
        }
        if over > 0
            && over <= inst.legalize_conflicts
            && round % 10 == 9
            && legalize_conflicts(inst, search, &mut routes, &mut count, rng)
        {
            eprintln!("construction strict repair resolved {} conflicts", over);
            over = 0;
        }
        if over == 0 {
            let total = routes.iter().map(|r| r.as_ref().unwrap().delay).sum();
            if best.is_none() {
                eprintln!("construction converged in {} rounds", round + 1);
            }
            if total < best_total {
                best_total = total;
                best = Some(routes.iter().map(|r| r.as_ref().unwrap().clone()).collect());
                eprintln!("construction best delay={} round={}", total, round + 1);
            }
            if negotiation_rounds == 0 {
                log_subset_counters(
                    subset_tree_max_sinks,
                    subset_attempts,
                    subset_candidates,
                    subset_kept_incumbent,
                    subset_skips,
                    subset_fallbacks,
                );
                return best;
            }
            // Release historical congestion scars and reconsider every net.
            // Retain the best complete legal layout before allowing overlaps.
            force_all = true;
            for price in &mut history {
                *price /= 2;
            }
            present = initial_present;
            continue;
        }
        if round % 20 == 19 {
            eprintln!("construction round {}: {} conflicts", round + 1, over);
        }
        // Keep present cost comparable to accumulated history. An enormous
        // present cost freezes oscillating pairs before history can divert them.
        present = if inst.slow_congestion {
            (present * 103 / 100).min(9216)
        } else {
            (present * 13 / 10).min(32768)
        };
    }
    log_subset_counters(
        subset_tree_max_sinks,
        subset_attempts,
        subset_candidates,
        subset_kept_incumbent,
        subset_skips,
        subset_fallbacks,
    );
    best
}
fn log_subset_counters(
    max_sinks: usize,
    attempts: usize,
    candidates: usize,
    kept_incumbent: usize,
    skips: usize,
    fallbacks: usize,
) {
    if max_sinks > 0 {
        eprintln!(
            "subset-tree summary max_sinks={} attempts={} exact_candidates={} kept_incumbent_ties={} skips={} configured_kernel_fallbacks={}",
            max_sinks, attempts, candidates, kept_incumbent, skips, fallbacks
        );
    }
}
fn improve(
    inst: &Instance,
    search: &mut Search,
    routes: &mut Vec<Route>,
    rng: &mut Rng,
    sweeps: usize,
    moves: usize,
) {
    let k = routes.len();
    let mut count = vec![0; inst.size];
    let history = vec![0; inst.size];
    for r in routes.iter() {
        occupy(&mut count, r, true);
    }
    let mut order: Vec<_> = (0..k).collect();
    for sweep in 0..sweeps {
        rng.shuffle(&mut order);
        let old: u64 = routes.iter().map(|r| r.delay).sum();
        for &i in &order {
            occupy(&mut count, &routes[i], false);
            let r = search
                .route(inst, i, &count, &history, 0, true, rng.next())
                .expect("old route proves reachability");
            assert!(r.delay <= routes[i].delay);
            routes[i] = r;
            occupy(&mut count, &routes[i], true);
        }
        eprintln!(
            "polish {}: {} -> {}",
            sweep + 1,
            old,
            routes.iter().map(|r| r.delay).sum::<u64>()
        );
    }
    for _ in 0..moves {
        let anchor = rng.next() as usize % k;
        let want = (2 + rng.next() as usize % 5).min(k);
        let mut candidates: Vec<_> = (0..k).filter(|&i| i != anchor).collect();
        rng.shuffle(&mut candidates);
        let av = inst.coord(inst.nets[anchor].driver);
        candidates.sort_by_key(|&i| {
            let v = inst.coord(inst.nets[i].driver);
            v[0].abs_diff(av[0]) + v[1].abs_diff(av[1])
        });
        let mut group = vec![anchor];
        group.extend(candidates.into_iter().take(want - 1));
        rng.shuffle(&mut group);
        let old: Vec<_> = group.iter().map(|&i| routes[i].clone()).collect();
        for r in &old {
            occupy(&mut count, r, false);
        }
        let mut fresh = Vec::new();
        for &i in &group {
            if let Some(r) = search.route(inst, i, &count, &history, 0, true, rng.next()) {
                occupy(&mut count, &r, true);
                fresh.push(r);
            } else {
                break;
            }
        }
        if fresh.len() == group.len()
            && fresh.iter().map(|r| r.delay).sum::<u64>() <= old.iter().map(|r| r.delay).sum()
        {
            for (&i, r) in group.iter().zip(fresh) {
                routes[i] = r;
            }
        } else {
            for r in &fresh {
                occupy(&mut count, r, false);
            }
            for r in &old {
                occupy(&mut count, r, true);
            }
        }
    }
    // Finish with exact single-net descent after group rearrangements.
    for _ in 0..2 {
        rng.shuffle(&mut order);
        for &i in &order {
            occupy(&mut count, &routes[i], false);
            let r = search
                .route(inst, i, &count, &history, 0, true, rng.next())
                .unwrap();
            assert!(r.delay <= routes[i].delay);
            routes[i] = r;
            occupy(&mut count, &routes[i], true);
        }
    }
    assert!(count.iter().all(|&c| c <= 1));
}
fn serialize_routes(inst: &Instance, routes: &[Route]) -> Vec<u8> {
    let output: Vec<_> = routes.iter().enumerate().map(|(i,r)| json!({
        "net": inst.nets[i].id,
        "edges": r.edges.iter().map(|&(a,b)| [inst.coord(a),inst.coord(b)]).collect::<Vec<_>>()
    })).collect();
    serde_json::to_vec(
        &json!({"format":"m3d-submission","version":1,"instance":inst.name,"routes":output}),
    )
    .unwrap()
}

fn adaptive_neighborhood_main_mode_allowed(mode: &str, trials: u64) -> bool {
    if trials == 0 {
        return false;
    }
    if matches!(mode, "mixed" | "mixed-joint") {
        return true;
    }
    matches!(
        mode,
        "portfolio" | "portfolio-joint" | "portfolio-joint-entry" | "portfolio-joint-polish"
    ) && trials / 2 > 0
}

fn adaptive_neighborhood_run_allowed(mode: &str, trials: u64, race_pilot: u64) -> bool {
    if race_pilot == 0 || race_pilot >= trials {
        return adaptive_neighborhood_main_mode_allowed(mode, trials);
    }
    adaptive_neighborhood_main_mode_allowed(mode, race_pilot)
        && adaptive_neighborhood_main_mode_allowed(mode, trials - race_pilot)
}
fn nonadaptive_phase_options(options: &refine::Options) -> Option<refine::Options> {
    if !options.adaptive_neighborhood {
        return None;
    }
    let mut phase_options = options.clone();
    phase_options.adaptive_neighborhood = false;
    Some(phase_options)
}

fn refine_output(
    instance: &str,
    output: &str,
    seed: u64,
    trials: u64,
    mode: &str,
    options: &refine::Options,
) -> u64 {
    assert!(trials > 0);
    let before = if mode == "portfolio"
        || mode == "portfolio-joint"
        || mode == "portfolio-cbs"
        || mode == "portfolio-joint-entry"
        || mode == "portfolio-joint-polish"
    {
        let first = trials / 2;
        if first > 0 {
            let stage = if mode == "portfolio-cbs" {
                "cbs-mixed"
            } else if mode == "portfolio-joint"
                || mode == "portfolio-joint-entry"
                || mode == "portfolio-joint-polish"
            {
                "mixed-joint"
            } else {
                "mixed"
            };
            refine::refine_cold(instance, output, output, seed, first, stage, options);
        }
        let chain_mode = if mode == "portfolio-joint-entry" {
            "chain-entry-fast"
        } else if mode == "portfolio-joint-polish" {
            "chain-polish-fast"
        } else {
            "chain-fast"
        };
        if let Some(chain_options) = nonadaptive_phase_options(options) {
            refine::refine_cold(
                instance,
                output,
                output,
                seed.wrapping_add(1),
                trials - first,
                chain_mode,
                &chain_options,
            )
        } else {
            refine::refine_cold(
                instance,
                output,
                output,
                seed.wrapping_add(1),
                trials - first,
                chain_mode,
                options,
            )
        }
    } else {
        refine::refine_cold(instance, output, output, seed, trials, mode, options)
    };
    let mut best = before;
    if options.post_cbs_trials > 0 {
        let mut post_options = options.clone();
        post_options.adaptive_neighborhood = false;
        let after = refine::refine_cold(
            instance,
            output,
            output,
            seed.wrapping_add(2),
            options.post_cbs_trials,
            "cbs-mixed",
            &post_options,
        );
        assert!(
            after <= before,
            "post-conflict search must retain its own incumbent"
        );
        best = after;
    }
    if options.post_cbs_bypass_trials > 0 {
        let mut bypass_options = options.clone();
        bypass_options.cbs_bypass = true;
        bypass_options.adaptive_neighborhood = false;
        let after = refine::refine_cold(
            instance,
            output,
            output,
            seed.wrapping_add(3),
            options.post_cbs_bypass_trials,
            "cbs-mixed",
            &bypass_options,
        );
        assert!(
            after <= best,
            "bypass pass must retain the completed stock incumbent"
        );
        best = after;
    }
    best
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: cold-spt-lns INSTANCE OUTPUT [--seed N --starts N --rounds N --sweeps N --moves N --layer-direction-preference 0|1|2]");
        std::process::exit(2);
    }
    let (mut seed, mut starts, mut rounds, mut sweeps, mut moves) =
        (20261022u64, 2usize, 200usize, 6usize, 128usize);
    let (mut refine_trials, mut refine_mode, mut construction_weight) =
        (0u64, String::from("chain-fast"), 1u64);
    let mut tree_root_weight = None;
    let mut negotiation_rounds = 0;
    let mut fanout_priority = 0;
    let mut flow_pricing = 0;
    let mut slow_congestion = false;
    let mut legalize_limit = 0;
    let mut use_cost_distance = false;
    let mut refine_each_start = false;
    let mut refine_forks = 1usize;
    let mut race_pilot = 0u64;
    let mut race_keep = 2usize;
    let mut cost_distance_rooted = false;
    let mut cost_distance_discount = false;
    let mut refine_options = refine::Options::default();
    let mut emit_global_pool = false;
    let mut final_stable_polish_passes = 0usize;
    let mut cross_profile_plan = 0u64;
    let mut subset_tree_max_sinks = 0usize;
    let mut layer_direction_preference = 0u64;
    for a in args[3..].chunks(2) {
        assert_eq!(a.len(), 2);
        match a[0].as_str() {
            "--seed" => seed = a[1].parse().unwrap(),
            "--starts" => starts = a[1].parse().unwrap(),
            "--rounds" => rounds = a[1].parse().unwrap(),
            "--sweeps" => sweeps = a[1].parse().unwrap(),
            "--moves" => moves = a[1].parse().unwrap(),
            "--refine-trials" => refine_trials = a[1].parse().unwrap(),
            "--refine-mode" => refine_mode = a[1].clone(),
            "--refine-temperature" => refine_options.temperature = a[1].parse().unwrap(),
            "--refine-slack" => refine_options.slack = a[1].parse().unwrap(),
            "--refine-group-max" => refine_options.group_max = a[1].parse().unwrap(),
            "--adaptive-neighborhood" => {
                refine_options.adaptive_neighborhood = a[1].parse::<u64>().unwrap() != 0
            }
            "--post-cbs-trials" => refine_options.post_cbs_trials = a[1].parse().unwrap(),
            "--post-cbs-bypass-trials" => {
                refine_options.post_cbs_bypass_trials = a[1].parse().unwrap()
            }
            "--cbs-nodes" => refine_options.cbs_nodes = a[1].parse().unwrap(),
            "--cbs-group-max" => refine_options.cbs_group_max = a[1].parse().unwrap(),
            "--cbs-conflict-priority" => {
                refine_options.cbs_conflict_priority = a[1].parse::<u64>().unwrap() != 0
            }
            "--joint-neutral" => refine_options.joint_neutral = a[1].parse().unwrap(),
            "--joint-group-max" => refine_options.joint_group_max = a[1].parse().unwrap(),
            "--retain-compact-best" => {
                refine_options.retain_compact_best = a[1].parse::<u64>().unwrap() != 0
            }
            "--penalized-astar" => {
                refine_options.penalized_astar = a[1].parse::<u64>().unwrap() != 0
            }
            "--penalized-integer-queue" => {
                refine_options.penalized_integer_queue = a[1].parse::<u64>().unwrap() != 0
            }
            "--penalized-heuristic-cache" => {
                refine_options.penalized_heuristic_cache = a[1].parse::<u64>().unwrap() != 0
            }
            "--emit-global-pool" => emit_global_pool = a[1].parse::<u64>().unwrap() != 0,
            "--compact-shortest-tree" => {
                refine_options.compact_shortest_tree = a[1].parse::<u64>().unwrap() != 0
            }
            "--compact-polish-passes" => {
                refine_options.compact_polish_passes = a[1].parse().unwrap()
            }
            "--final-stable-polish-passes" => final_stable_polish_passes = a[1].parse().unwrap(),
            "--cross-profile-plan" => cross_profile_plan = a[1].parse().unwrap(),
            "--subset-tree-max-sinks" => subset_tree_max_sinks = a[1].parse().unwrap(),
            "--layer-direction-preference" => layer_direction_preference = a[1].parse().unwrap(),
            "--construction-weight" => construction_weight = a[1].parse().unwrap(),
            "--tree-root-weight" => tree_root_weight = Some(a[1].parse::<u64>().unwrap()),
            "--negotiation-rounds" => negotiation_rounds = a[1].parse::<usize>().unwrap(),
            "--fanout-priority" => fanout_priority = a[1].parse::<u64>().unwrap(),
            "--flow-pricing" => flow_pricing = a[1].parse::<u64>().unwrap(),
            "--slow-congestion" => slow_congestion = a[1].parse::<u64>().unwrap() != 0,
            "--legalize-conflicts" => legalize_limit = a[1].parse::<usize>().unwrap(),
            "--cost-distance" => use_cost_distance = a[1].parse::<u64>().unwrap() != 0,
            "--refine-each-start" => refine_each_start = a[1].parse::<u64>().unwrap() != 0,
            "--refine-forks" => refine_forks = a[1].parse().unwrap(),
            "--refine-race-pilot" => race_pilot = a[1].parse().unwrap(),
            "--refine-race-keep" => race_keep = a[1].parse().unwrap(),
            "--cost-distance-rooted" => cost_distance_rooted = a[1].parse::<u64>().unwrap() != 0,
            "--cost-distance-discount" => {
                cost_distance_discount = a[1].parse::<u64>().unwrap() != 0
            }
            _ => panic!("unknown argument {}", a[0]),
        }
    }
    assert!(starts > 0 && rounds > 0);
    assert!(cross_profile_plan <= 2);
    assert!(subset_tree_max_sinks == 0 || (2..=6).contains(&subset_tree_max_sinks));
    assert!(refine_forks > 0);
    assert!(final_stable_polish_passes <= 64);
    let walk_count = starts
        .checked_mul(refine_forks)
        .expect("refinement walk count overflow");
    assert!(refine_forks == 1 || (refine_each_start && refine_trials > 0));
    assert!(construction_weight >= 1);
    assert!(
        layer_direction_preference <= 2,
        "layer direction preference must be 0, 1, or 2"
    );
    assert!(
        direction_mode_compatible(
            layer_direction_preference,
            use_cost_distance,
            tree_root_weight,
            subset_tree_max_sinks,
            cross_profile_plan,
            flow_pricing,
        ),
        "layer direction preference requires base SPT construction and is incompatible with cost-distance, tree-root weighting, subset trees, cross-profile planning, and flow pricing"
    );
    assert!(subset_tree_max_sinks == 0 || (!use_cost_distance && tree_root_weight.is_none() && flow_pricing == 0 && cross_profile_plan == 0), "subset tree kernel is incompatible with cost-distance, tree-root weighting, flow pricing, and cross-profile planning");
    assert!(refine_options.post_cbs_bypass_trials == 0 || refine_trials > 0);
    assert!(
        !refine_options.adaptive_neighborhood
            || adaptive_neighborhood_run_allowed(&refine_mode, refine_trials, race_pilot),
        "adaptive neighborhood requires direct mixed/mixed-joint mode or a portfolio mode with a nonempty mixed/mixed-joint first phase"
    );
    if race_pilot > 0 {
        assert!(refine_each_start && race_pilot <= refine_trials);
        assert!(race_keep > 0 && race_keep <= walk_count);
        assert!(
            race_pilot < refine_trials
                || (refine_options.post_cbs_trials == 0
                    && refine_options.post_cbs_bypass_trials == 0)
        );
    }
    let mut inst = Instance::load(&args[1]);
    inst.construction_weight = construction_weight;
    assert!(fanout_priority <= 2);
    inst.fanout_priority = fanout_priority;
    assert!(flow_pricing <= 2);
    assert!(flow_pricing == 0 || (!use_cost_distance && tree_root_weight.is_none()));
    inst.flow_pricing = flow_pricing;
    inst.slow_congestion = slow_congestion;
    inst.legalize_conflicts = legalize_limit;
    inst.cost_distance = use_cost_distance;
    inst.cost_distance_rooted = cost_distance_rooted;
    inst.cost_distance_discount = cost_distance_discount;
    if cross_profile_plan > 0 {
        plan_cross_profiles(&mut inst, cross_profile_plan, seed);
    }
    let mut search = Search::new(inst.size);
    search.layer_direction_preference = layer_direction_preference;
    let mut rng = Rng(seed);
    let mut best: Option<Vec<Route>> = None;
    let mut best_final = None;
    let mut best_final_score = u64::MAX;
    // Walks can share this invocation's fresh cold construction. Only its own
    // completed pilot bytes can become finalists; there is no route input.
    let mut pilots: Vec<(u64, usize, Vec<u8>)> = Vec::new();
    for start in 0..starts {
        search.direction_phase_stats = DirectionPhaseStats::default();
        eprintln!("cold start {} of {}", start + 1, starts);
        let candidate = build(
            &inst,
            &mut search,
            &mut rng,
            rounds,
            tree_root_weight,
            negotiation_rounds,
            subset_tree_max_sinks,
        );
        let direction_exhausted = candidate.is_none() && layer_direction_preference != 0;
        let retry_uniform = should_retry_uniform(
            candidate.is_none(),
            inst.slow_congestion
                || inst.cost_distance
                || inst.fanout_priority != 0
                || inst.flow_pricing != 0,
            subset_tree_max_sinks,
            layer_direction_preference != 0,
        );
        let fallback_attempted = retry_uniform;
        let (candidate, fallback_selected) = if retry_uniform {
            eprintln!("construction experimental attempt exhausted; fresh uniform SPT fallback");
            let settings = (
                inst.slow_congestion,
                inst.fanout_priority,
                inst.construction_weight,
                inst.cost_distance,
                inst.flow_pricing,
            );
            inst.slow_congestion = false;
            inst.fanout_priority = 0;
            inst.construction_weight = 2;
            inst.cost_distance = false;
            inst.flow_pricing = 0;
            let fallback = with_uniform_direction_preference(&mut search, |search| {
                build(&inst, search, &mut rng, 1000, None, negotiation_rounds, 0)
            });
            (
                inst.slow_congestion,
                inst.fanout_priority,
                inst.construction_weight,
                inst.cost_distance,
                inst.flow_pricing,
            ) = settings;
            let selected = fallback.is_some();
            (fallback, selected)
        } else {
            (candidate, false)
        };
        if layer_direction_preference == 2 {
            let stats = &search.direction_phase_stats;
            eprintln!(
                "layer-direction-phase-summary mode=2 fallback_attempted={} fallback_selected={} guidance_active={} paired_calls={} phase0_selected={} phase1_selected={} phase0_failed={} phase1_failed={} both_failed={} comparable_calls={} common_objective_delta_phase1_minus_phase0={}",
                fallback_attempted,
                fallback_selected,
                !fallback_selected && !direction_exhausted,
                stats.paired_calls,
                stats.phase0_selected,
                stats.phase1_selected,
                stats.phase0_failed,
                stats.phase1_failed,
                stats.both_failed,
                stats.comparable_calls,
                stats.common_delta_phase1_minus_phase0
            );
        }
        if let Some(mut routes) = candidate {
            if layer_direction_preference == 1 {
                let stats = direction_telemetry(&inst, &routes);
                eprintln!(
                    "layer-direction-raw enabled=true mode={} profile=alternating-even-x-odd-y fallback={} preferred={} nonpreferred={} vias={} central_cut={} by_layer={:?}",
                    if direction_exhausted { 0 } else { layer_direction_preference },
                    direction_exhausted,
                    stats.preferred,
                    stats.nonpreferred,
                    stats.vias,
                    stats.central_cut,
                    stats.by_layer,
                );
            }
            if cross_profile_plan > 0 {
                report_profile_adherence(&inst, &routes);
            }
            if sweeps > 0 || moves > 0 {
                improve(&inst, &mut search, &mut routes, &mut rng, sweeps, moves);
            }
            let score: u64 = routes.iter().map(|r| r.delay).sum();
            eprintln!("start {} delay {}", start + 1, score);
            if refine_each_start && refine_trials > 0 {
                // Every fork resets to the same freshly generated cold bytes.
                // All construction and walks remain inside this timed process.
                let cold_bytes = serialize_routes(&inst, &routes);
                for fork in 0..refine_forks {
                    fs::write(&args[2], &cold_bytes).expect("write fresh cold output");
                    let walk = start * refine_forks + fork;
                    let start_seed =
                        seed.wrapping_add((walk as u64).wrapping_mul(0x9E3779B97F4A7C15));
                    let mut pilot_options = refine_options.clone();
                    if race_pilot > 0 {
                        pilot_options.post_cbs_trials = 0;
                        pilot_options.post_cbs_bypass_trials = 0;
                    }
                    let final_score = refine_output(
                        &args[1],
                        &args[2],
                        start_seed,
                        if race_pilot > 0 {
                            race_pilot
                        } else {
                            refine_trials
                        },
                        &refine_mode,
                        &pilot_options,
                    );
                    eprintln!(
                        "completed cold start {} fork {} final delay {}",
                        start + 1,
                        fork + 1,
                        final_score
                    );
                    if final_score < best_final_score {
                        best_final_score = final_score;
                        best_final =
                            Some(fs::read(&args[2]).expect("read this invocation final output"));
                    }
                    if race_pilot > 0 {
                        pilots.push((
                            final_score,
                            walk,
                            fs::read(&args[2]).expect("read own pilot output"),
                        ));
                    }
                }
                continue;
            }
            if best
                .as_ref()
                .map_or(true, |r| score < r.iter().map(|r| r.delay).sum())
            {
                best = Some(routes);
            }
        }
    }
    if race_pilot > 0 && race_pilot < refine_trials {
        pilots.sort_by_key(|p| (p.0, p.1));
        pilots.truncate(race_keep);
        for (pilot_score, start, bytes) in pilots {
            fs::write(&args[2], bytes).expect("write this invocation's own finalist");
            let final_seed = seed
                .wrapping_add((start as u64).wrapping_mul(0x9E3779B97F4A7C15))
                .wrapping_add(0xA51C39E70D682F43);
            eprintln!(
                "racing finalist start {} pilot={} remaining_trials={}",
                start + 1,
                pilot_score,
                refine_trials - race_pilot
            );
            let final_score = refine_output(
                &args[1],
                &args[2],
                final_seed,
                refine_trials - race_pilot,
                &refine_mode,
                &refine_options,
            );
            assert!(
                final_score <= pilot_score,
                "finalist must retain its own pilot incumbent"
            );
            if final_score < best_final_score {
                best_final_score = final_score;
                best_final = Some(fs::read(&args[2]).expect("read own refined finalist"));
            }
        }
    }
    if let Some(bytes) = best_final {
        fs::write(&args[2], bytes).expect("write best internally generated final output");
        let final_score = if final_stable_polish_passes > 0 {
            refine::stable_polish_output(&args[1], &args[2], final_stable_polish_passes)
        } else {
            best_final_score
        };
        println!("total_delay={final_score}");
        if emit_global_pool {
            refine::write_pool(&args[1], &args[2], &format!("{}.pool.json", args[2]), seed);
        }
        return;
    }
    let routes = best.expect("no legal cold construction; no output written");
    let total: u64 = routes.iter().map(|r| r.delay).sum();
    fs::write(&args[2], serialize_routes(&inst, &routes)).expect("write output");
    println!("total_delay={total}");
    if refine_trials > 0 {
        drop(search);
        drop(routes);
        drop(inst);
        refine_output(
            &args[1],
            &args[2],
            seed,
            refine_trials,
            &refine_mode,
            &refine_options,
        );
    }
    if final_stable_polish_passes > 0 {
        let final_score =
            refine::stable_polish_output(&args[1], &args[2], final_stable_polish_passes);
        println!("total_delay={final_score}");
    }
    if emit_global_pool {
        refine::write_pool(&args[1], &args[2], &format!("{}.pool.json", args[2]), seed);
    }
}

fn should_retry_uniform(
    candidate_missing: bool,
    other_experimental_mode: bool,
    subset_tree_max_sinks: usize,
    layer_direction_preference: bool,
) -> bool {
    candidate_missing
        && (other_experimental_mode || subset_tree_max_sinks != 0 || layer_direction_preference)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_neighborhood_portfolio_phase_eligibility_is_narrow() {
        for mode in [
            "portfolio",
            "portfolio-joint",
            "portfolio-joint-entry",
            "portfolio-joint-polish",
        ] {
            assert!(adaptive_neighborhood_main_mode_allowed(mode, 2), "{mode}");
            assert!(!adaptive_neighborhood_main_mode_allowed(mode, 1), "{mode}");
        }
        for mode in [
            "portfolio-cbs",
            "cbs",
            "cbs-mixed",
            "chain-fast",
            "joint",
            "single",
        ] {
            assert!(!adaptive_neighborhood_main_mode_allowed(mode, 100), "{mode}");
        }
        assert!(adaptive_neighborhood_main_mode_allowed("mixed", 1));
        assert!(adaptive_neighborhood_main_mode_allowed("mixed-joint", 1));
        assert!(!adaptive_neighborhood_main_mode_allowed("mixed", 0));
    }

    #[test]
    fn default_nonadaptive_phase_uses_original_options_without_clone() {
        let options = refine::Options::default();
        assert!(nonadaptive_phase_options(&options).is_none());
    }
    #[test]
    fn adaptive_portfolio_race_segments_each_need_a_nonempty_main_phase() {
        assert!(!adaptive_neighborhood_run_allowed("portfolio-joint", 2, 1));
        assert!(adaptive_neighborhood_run_allowed("portfolio-joint", 4, 2));
        assert!(adaptive_neighborhood_run_allowed("portfolio", 3, 3));
        assert!(!adaptive_neighborhood_run_allowed("portfolio", 5, 4));
        assert!(adaptive_neighborhood_run_allowed("mixed", 2, 1));
        assert!(adaptive_neighborhood_run_allowed("mixed-joint", 2, 1));
    }
    #[test]
    fn adaptive_chain_clone_disables_only_the_opt_in_flag() {
        let mut options = refine::Options::default();
        options.adaptive_neighborhood = true;
        options.temperature = 19;
        let chain_options = nonadaptive_phase_options(&options).unwrap();
        assert!(options.adaptive_neighborhood);
        assert!(!chain_options.adaptive_neighborhood);
        assert_eq!(chain_options.temperature, options.temperature);
    }
    fn directional_fixture() -> Instance {
        let (w, h, layers) = (3usize, 3usize, 2usize);
        let wh = w * h;
        let size = wh * layers;
        let vertex = |x: usize, y: usize, z: usize| z * wh + y * w + x;
        let driver = vertex(0, 0, 0);
        let sink = vertex(2, 2, 1);
        let mut pins = vec![-1; size];
        pins[driver] = 0;
        pins[sink] = 0;
        let mut neighbors = vec![vec![]; size];
        for u in 0..size {
            let (z, r) = (u / wh, u % wh);
            let (y, x) = (r / w, r % w);
            if x + 1 < w {
                neighbors[u].push((u + 1, 1));
            }
            if x > 0 {
                neighbors[u].push((u - 1, 1));
            }
            if y + 1 < h {
                neighbors[u].push((u + w, 1));
            }
            if y > 0 {
                neighbors[u].push((u - w, 1));
            }
            if z + 1 < layers {
                neighbors[u].push((u + wh, 3));
            }
            if z > 0 {
                neighbors[u].push((u - wh, 3));
            }
        }
        Instance {
            flow_pricing: 0,
            construction_weight: 1,
            fanout_priority: 0,
            slow_congestion: false,
            legalize_conflicts: 0,
            cost_distance: false,
            cost_distance_rooted: false,
            cost_distance_discount: false,
            name: "directional-synthetic".into(),
            w,
            h,
            wh,
            size,
            ld: vec![1, 1],
            via: 3,
            nets: vec![Net {
                id: 1,
                driver,
                sinks: vec![sink],
                cross_singleton: false,
            }],
            cross_profiles: vec![None],
            pins,
            neighbors,
            free_distance: vec![],
        }
    }

    fn single_layer_fixture(sink_x: usize, sink_y: usize, via: u64) -> Instance {
        let w = sink_x.max(1) + 1;
        let h = sink_y.max(1) + 1;
        let wh = w * h;
        let driver = 0usize;
        let sink = sink_y * w + sink_x;
        let mut pins = vec![-1; wh];
        pins[driver] = 0;
        pins[sink] = 0;
        let mut neighbors = vec![Vec::new(); wh];
        for y in 0..h {
            for x in 0..w {
                let u = y * w + x;
                if x + 1 < w {
                    neighbors[u].push((u + 1, 1));
                }
                if x > 0 {
                    neighbors[u].push((u - 1, 1));
                }
                if y + 1 < h {
                    neighbors[u].push((u + w, 1));
                }
                if y > 0 {
                    neighbors[u].push((u - w, 1));
                }
            }
        }
        Instance {
            flow_pricing: 0,
            construction_weight: 1,
            fanout_priority: 0,
            slow_congestion: false,
            legalize_conflicts: 0,
            cost_distance: false,
            cost_distance_rooted: false,
            cost_distance_discount: false,
            name: "direction-phase-single-layer".into(),
            w,
            h,
            wh,
            size: wh,
            ld: vec![1],
            via,
            nets: vec![Net {
                id: 4,
                driver,
                sinks: vec![sink],
                cross_singleton: false,
            }],
            cross_profiles: vec![None],
            pins,
            neighbors,
            free_distance: vec![],
        }
    }

    #[test]
    fn directional_penalty_alternates_axes_and_leaves_vias_unbiased() {
        let inst = directional_fixture();
        let v = |x, y, z| (z * inst.h + y) * inst.w + x;
        assert_eq!(
            lateral_direction_penalty(&inst, v(0, 0, 0), v(1, 0, 0), 1),
            Some(0)
        );
        assert_eq!(
            lateral_direction_penalty(&inst, v(0, 0, 0), v(0, 1, 0), 1),
            Some(3072)
        );
        assert_eq!(
            lateral_direction_penalty(&inst, v(0, 0, 1), v(0, 1, 1), 1),
            Some(0)
        );
        assert_eq!(
            lateral_direction_penalty(&inst, v(0, 0, 1), v(1, 0, 1), 1),
            Some(3072)
        );
        assert_eq!(
            lateral_direction_penalty(&inst, v(0, 0, 0), v(0, 0, 1), 1),
            Some(0)
        );
        assert_eq!(
            lateral_direction_penalty(&inst, v(0, 0, 0), v(0, 1, 0), 0),
            Some(0)
        );
        assert_eq!(
            lateral_direction_penalty(&inst, v(0, 0, 0), v(0, 1, 0), 2),
            Some(0)
        );
        assert_eq!(
            lateral_direction_penalty(&inst, v(0, 0, 0), v(1, 0, 0), 2),
            Some(3072)
        );
    }

    #[test]
    fn directional_penalty_is_checked_for_overflow() {
        let mut inst = directional_fixture();
        inst.via = u64::MAX;
        assert_eq!(lateral_direction_penalty(&inst, 0, 3, 1), None);
        assert_eq!(construction_search_step(u64::MAX, 0, 0), None);
        assert_eq!(construction_search_step(1, u64::MAX, 1), None);
    }

    #[test]
    fn soft_preference_keeps_nonpreferred_moves_feasible() {
        let mut inst = directional_fixture();
        inst.nets[0].sinks = vec![3]; // One Y edge on even layer zero.
        let mut search = Search::new(inst.size);
        search.layer_direction_preference = 1;
        assert!(search
            .route(
                &inst,
                0,
                &vec![0; inst.size],
                &vec![0; inst.size],
                0,
                false,
                7
            )
            .is_some());
    }

    #[test]
    fn strict_route_ignores_direction_preference_and_retains_physical_optimum() {
        let inst = directional_fixture();
        let (count, history) = (vec![0; inst.size], vec![0; inst.size]);
        let mut uniform = Search::new(inst.size);
        let mut preferred = Search::new(inst.size);
        preferred.layer_direction_preference = 1;
        let mut adaptive = Search::new(inst.size);
        adaptive.layer_direction_preference = 2;
        let a = uniform
            .route(&inst, 0, &count, &history, 0, true, 19)
            .unwrap();
        let b = preferred
            .route(&inst, 0, &count, &history, 0, true, 19)
            .unwrap();
        let c = adaptive
            .route(&inst, 0, &count, &history, 0, true, 19)
            .unwrap();
        assert_eq!(a.delay, b.delay);
        assert_eq!(a.edges, b.edges);
        assert_eq!(a.delay, c.delay);
        assert_eq!(a.edges, c.edges);
    }

    #[test]
    fn construction_chooses_complementary_axes_on_adjacent_layers() {
        let inst = directional_fixture();
        let mut search = Search::new(inst.size);
        search.layer_direction_preference = 1;
        let route = search
            .route(
                &inst,
                0,
                &vec![0; inst.size],
                &vec![0; inst.size],
                0,
                false,
                29,
            )
            .unwrap();
        assert!(route
            .edges
            .iter()
            .any(|&(a, b)| inst.coord(a)[2] == 0 && inst.coord(a)[0] != inst.coord(b)[0]));
        assert!(route
            .edges
            .iter()
            .any(|&(a, b)| inst.coord(a)[2] == 1 && inst.coord(a)[1] != inst.coord(b)[1]));
        let stats = direction_telemetry(&inst, &[route]);
        assert!(stats.preferred >= 4);
        assert_eq!(stats.nonpreferred, 0);
    }

    #[test]
    fn direction_mode_rejects_incompatible_construction_kernels() {
        assert!(direction_mode_compatible(0, true, Some(50), 3, 1, 1));
        assert!(direction_mode_compatible(1, false, None, 0, 0, 0));
        assert!(direction_mode_compatible(2, false, None, 0, 0, 0));
        assert!(!direction_mode_compatible(3, false, None, 0, 0, 0));
        assert!(!direction_mode_compatible(1, true, None, 0, 0, 0));
        assert!(!direction_mode_compatible(1, false, Some(50), 0, 0, 0));
        assert!(!direction_mode_compatible(1, false, None, 2, 0, 0));
        assert!(!direction_mode_compatible(1, false, None, 0, 1, 0));
        assert!(!direction_mode_compatible(1, false, None, 0, 0, 1));
        assert!(!direction_mode_compatible(2, true, None, 0, 0, 0));
        assert!(!direction_mode_compatible(2, false, Some(50), 0, 0, 0));
        assert!(!direction_mode_compatible(2, false, None, 2, 0, 0));
        assert!(!direction_mode_compatible(2, false, None, 0, 1, 0));
        assert!(!direction_mode_compatible(2, false, None, 0, 0, 1));
    }

    #[test]
    fn common_score_and_selector_choose_phase1_only_for_strictly_lower_score() {
        let inst = directional_fixture();
        let root = inst.nets[0].driver;
        let high_price = (0 * inst.wh) + 1; // (1, 0, 0)
        let low_price = (0 * inst.wh) + 3; // (0, 1, 0)
        let mut history = vec![0; inst.size];
        history[high_price] = 50;
        let count = vec![0; inst.size];
        let phase0 = Route {
            vertices: vec![root, high_price],
            edges: vec![(root, high_price)],
            delay: 2,
        };
        let phase1 = Route {
            vertices: vec![root, low_price],
            edges: vec![(root, low_price)],
            delay: 2,
        };
        let score0 = common_conditional_route_score(&inst, 0, &phase0, &count, &history, 17);
        let score1 = common_conditional_route_score(&inst, 0, &phase1, &count, &history, 17);
        assert!(score1 < score0);
        let (selected, phase, delta) =
            choose_direction_candidate(Some((phase0.clone(), score0)), Some((phase1, score1)));
        assert_eq!(phase, Some(1));
        assert_eq!(delta, Some(score1 as i128 - score0 as i128));
        assert_eq!(
            selected.unwrap().edges,
            vec![(root.min(low_price), root.max(low_price))]
        );
        let (tie_selected, tie_phase, tie_delta) = choose_direction_candidate(
            Some((phase0.clone(), score0)),
            Some((phase0.clone(), score0)),
        );
        assert_eq!(tie_selected.unwrap().edges, phase0.edges);
        assert_eq!(tie_phase, Some(0));
        assert_eq!(tie_delta, Some(0));
    }

    #[test]
    fn paired_queries_keep_the_same_occupancy_and_history_and_can_select_phase1() {
        let inst = directional_fixture();
        let mut search = Search::new(inst.size);
        search.layer_direction_preference = 1;
        let empty_count = vec![0; inst.size];
        let empty_history = vec![0; inst.size];
        let baseline = search
            .route(&inst, 0, &empty_count, &empty_history, 0, false, 41)
            .unwrap();
        let mut count = vec![0; inst.size];
        let mut history = vec![0; inst.size];
        for &vertex in &baseline.vertices {
            if vertex != inst.nets[0].driver && !inst.nets[0].sinks.contains(&vertex) {
                history[vertex] = 25;
            }
        }
        let count_before = count.clone();
        let history_before = history.clone();
        search.layer_direction_preference = 2;
        let selected = paired_direction_route(&inst, &mut search, 0, &count, &history, 0, 41);
        assert!(selected.is_some());
        assert_eq!(count, count_before);
        assert_eq!(history, history_before);
        assert_eq!(search.layer_direction_phase, 0);
        assert_eq!(search.direction_phase_stats.paired_calls, 1);
        assert_eq!(search.direction_phase_stats.comparable_calls, 1);
        assert_eq!(search.direction_phase_stats.phase1_selected, 1);
        assert!(
            search
                .direction_phase_stats
                .common_delta_phase1_minus_phase0
                < 0
        );
    }

    #[test]
    fn mode2_uses_the_only_valid_phase_when_one_query_exhausts() {
        let inst = single_layer_fixture(0, 1, u64::MAX);
        let (count, history) = (vec![0; inst.size], vec![0; inst.size]);
        let mut search = Search::new(inst.size);
        search.layer_direction_preference = 2;
        let route = paired_direction_route(&inst, &mut search, 0, &count, &history, 0, 17);
        assert!(route.is_some());
        assert_eq!(search.direction_phase_stats.phase0_failed, 1);
        assert_eq!(search.direction_phase_stats.phase1_failed, 0);
        assert_eq!(search.direction_phase_stats.phase1_selected, 1);
    }

    #[test]
    fn both_phase_failures_take_uniform_fallback_and_keep_a_legal_route() {
        let inst = single_layer_fixture(1, 1, u64::MAX);
        let (count, history) = (vec![0; inst.size], vec![0; inst.size]);
        let mut search = Search::new(inst.size);
        search.layer_direction_preference = 2;
        let paired = paired_direction_route(&inst, &mut search, 0, &count, &history, 0, 73);
        assert!(paired.is_none());
        assert_eq!(search.direction_phase_stats.both_failed, 1);
        assert!(should_retry_uniform(true, false, 0, true));
        let fallback = with_uniform_direction_preference(&mut search, |uniform| {
            assert_eq!(uniform.layer_direction_preference, 0);
            uniform.route(&inst, 0, &count, &history, 0, false, 73)
        });
        let route = fallback.expect("uniform mode recovers the synthetic route");
        assert_eq!(search.layer_direction_preference, 2);
        assert!(route.vertices.contains(&inst.nets[0].driver));
        assert!(route.vertices.contains(&inst.nets[0].sinks[0]));
        assert_eq!(route.edges.len() + 1, route.vertices.len());
        for &(a, b) in &route.edges {
            let ca = inst.coord(a);
            let cb = inst.coord(b);
            assert_eq!(
                ca[0].abs_diff(cb[0]) + ca[1].abs_diff(cb[1]) + ca[2].abs_diff(cb[2]),
                1
            );
        }
    }

    #[test]
    fn enabled_subset_does_not_discard_a_successful_candidate() {
        assert!(!should_retry_uniform(false, false, 4, false));
        assert!(!should_retry_uniform(false, true, 4, false));
        assert!(should_retry_uniform(true, false, 4, false));
        assert!(should_retry_uniform(true, true, 0, false));
        assert!(!should_retry_uniform(false, false, 0, true));
        assert!(should_retry_uniform(true, false, 0, true));
    }

    #[test]
    fn directional_fallback_trigger_is_isolated_to_missing_candidates() {
        assert!(!should_retry_uniform(false, false, 0, true));
        assert!(should_retry_uniform(true, false, 0, true));
        assert!(!should_retry_uniform(false, false, 0, false));
        let mut search = Search::new(1);
        search.layer_direction_preference = 1;
        let result = with_uniform_direction_preference(&mut search, |fallback_search| {
            assert_eq!(fallback_search.layer_direction_preference, 0);
            42
        });
        assert_eq!(result, 42);
        assert_eq!(search.layer_direction_preference, 1);
    }

    fn profile_fixture() -> Instance {
        let (w, h, layers) = (5usize, 5usize, 6usize);
        let wh = w * h;
        let size = wh * layers;
        let vertex = |x: usize, y: usize, z: usize| z * wh + y * w + x;
        let mut inst = Instance {
            flow_pricing: 0,
            construction_weight: 1,
            fanout_priority: 0,
            slow_congestion: false,
            legalize_conflicts: 0,
            cost_distance: false,
            cost_distance_rooted: false,
            cost_distance_discount: false,
            name: "cross-profile-fixture".into(),
            w,
            h,
            wh,
            size,
            ld: vec![6, 4, 2, 2, 4, 6],
            via: 3,
            nets: vec![
                Net {
                    id: 7,
                    driver: vertex(0, 1, 0),
                    sinks: vec![vertex(3, 2, 5)],
                    cross_singleton: true,
                },
                Net {
                    id: 12,
                    driver: vertex(4, 4, 0),
                    sinks: vec![vertex(3, 4, 0)],
                    cross_singleton: false,
                },
            ],
            cross_profiles: vec![None; 2],
            pins: vec![-1; size],
            neighbors: vec![vec![]; size],
            free_distance: Vec::new(),
        };
        for (nid, n) in inst.nets.iter().enumerate() {
            inst.pins[n.driver] = nid as i32;
            for &sink in &n.sinks {
                inst.pins[sink] = nid as i32;
            }
        }
        for u in 0..size {
            let c = inst.coord(u);
            for (dx, dy, dz) in [(1isize, 0isize, 0isize), (0, 1, 0), (0, 0, 1)] {
                let x = c[0] as isize + dx;
                let y = c[1] as isize + dy;
                let z = c[2] as isize + dz;
                if x < w as isize && y < h as isize && z < layers as isize {
                    let v = vertex(x as usize, y as usize, z as usize);
                    let cost = inst.weight(u, v);
                    inst.neighbors[u].push((v, cost));
                    inst.neighbors[v].push((u, cost));
                }
            }
        }
        inst
    }

    #[test]
    fn cross_profiles_are_legal_exact_lb_3d_paths_with_via_columns() {
        let inst = profile_fixture();
        let net = &inst.nets[0];
        let manhattan = 4u64;
        for plane in [2usize, 3] {
            for elbow in [0usize, 1] {
                let p = profile_for(&inst, 0, plane, elbow);
                assert_eq!(p.delay, 15 + 2 * manhattan);
                assert_eq!(p.delay, inst.lower_bound(net.driver, &net.sinks));
                assert_eq!(p.edges.len() + 1, p.vertices.len());
                assert!(p.vertices.binary_search(&net.driver).is_ok());
                assert!(p.vertices.binary_search(&net.sinks[0]).is_ok());
                assert!(p
                    .vertices
                    .iter()
                    .all(|&v| inst.pins[v] < 0 || inst.pins[v] == 0));
                let via_edges: Vec<_> = p
                    .edges
                    .iter()
                    .filter(|&&(a, b)| a / inst.wh != b / inst.wh)
                    .collect();
                assert_eq!(via_edges.len(), 5);
                for &&(a, b) in &via_edges {
                    let ca = inst.coord(a);
                    let cb = inst.coord(b);
                    assert_eq!((ca[0], ca[1]), (cb[0], cb[1]));
                }
                for &(a, b) in &p.edges {
                    assert!(inst.neighbors[a].iter().any(|&(v, _)| v == b));
                }
            }
        }
    }

    #[test]
    fn raw_profile_planner_uses_shared_balanced_initialization_and_assigns_only_targets() {
        assert_eq!(
            (0..4)
                .map(balanced_initial_profile_choice)
                .collect::<Vec<_>>(),
            vec![0, 2, 0, 2]
        );
        let mut mode1 = profile_fixture();
        plan_cross_profiles(&mut mode1, 1, 20261022);
        assert!(mode1.cross_profiles[0].is_some());
        assert!(mode1.cross_profiles[1].is_none());
        let p1 = profile_for(&mode1, 0, 2, 0);
        assert_eq!(mode1.cross_profiles[0].as_ref(), Some(&p1.vertices));
        let mut mode2 = profile_fixture();
        plan_cross_profiles(&mut mode2, 2, 20261022);
        assert!(mode2.cross_profiles[0].is_some());
        assert!(mode2.cross_profiles[1].is_none());
        assert_eq!(
            mode2.cross_profiles[0].as_ref().unwrap().first(),
            Some(&mode2.nets[0].driver)
        );
        assert_eq!(
            mode2.cross_profiles[0].as_ref().unwrap().last(),
            Some(&mode2.nets[0].sinks[0])
        );
    }

    #[test]
    fn relaxed_profile_price_favors_profile_but_strict_route_ignores_it() {
        let mut inst = profile_fixture();
        let selected = profile_for(&inst, 0, 2, 0);
        let count = vec![0; inst.size];
        let history = vec![0; inst.size];
        let salt = 91;
        let strict_before = Search::new(inst.size)
            .route(&inst, 0, &count, &history, 0, true, salt)
            .unwrap();
        inst.cross_profiles[0] = Some(selected.vertices.clone());
        let strict_after = Search::new(inst.size)
            .route(&inst, 0, &count, &history, 0, true, salt)
            .unwrap();
        assert_eq!(strict_before.edges, strict_after.edges);
        let relaxed = Search::new(inst.size)
            .route(&inst, 0, &count, &history, 0, false, salt)
            .unwrap();
        assert_eq!(relaxed.edges, selected.edges);
        assert_eq!(relaxed.delay, selected.delay);
    }

    #[test]
    fn shared_resource_flow_counts_internal_sinks_and_resets_between_nets() {
        let mut search = Search::new(8);
        let net = Net {
            id: 0,
            driver: 0,
            sinks: vec![1, 3, 5],
            cross_singleton: false,
        };
        let old = Route {
            vertices: vec![0, 1, 2, 3, 4, 5],
            edges: vec![(0, 1), (1, 2), (2, 3), (1, 4), (4, 5)],
            delay: 7,
        };
        search.prepare_flow(&net, Some(&old));
        assert_eq!(&search.flow[..6], &[3, 3, 1, 1, 1, 1]);
        assert_eq!(search.flow_price(1, 1, 1200), 400);
        assert_eq!(search.flow_price(1, 3, 1200), 1200);
        assert_eq!(search.flow_price(1, 7, 1200), 1200);
        assert_eq!(search.flow_price(2, 1, 1200), 693);
        search.prepare_flow(
            &Net {
                id: 1,
                driver: 6,
                sinks: vec![7],
                cross_singleton: false,
            },
            None,
        );
        assert_eq!(search.flow_price(1, 1, 1200), 1200);
    }
    #[test]
    fn strict_repair_resolves_crossings_or_restores_the_incumbent() {
        for layers in [1, 2] {
            let mut inst = Instance {
                flow_pricing: 0,
                construction_weight: 1,
                fanout_priority: 0,
                slow_congestion: false,
                legalize_conflicts: 128,
                cost_distance: false,
                cost_distance_rooted: false,
                cost_distance_discount: false,
                name: "crossings".into(),
                w: 3,
                h: 2,
                wh: 6,
                size: 6 * layers,
                ld: vec![1; layers],
                via: 2,
                nets: vec![
                    Net {
                        id: 0,
                        driver: 0,
                        sinks: vec![5],
                        cross_singleton: false,
                    },
                    Net {
                        id: 1,
                        driver: 2,
                        sinks: vec![3],
                        cross_singleton: false,
                    },
                ],
                cross_profiles: vec![None; 6 * layers],
                pins: vec![-1; 6 * layers],
                neighbors: vec![vec![]; 6 * layers],
                free_distance: Vec::new(),
            };
            for (i, net) in inst.nets.iter().enumerate() {
                inst.pins[net.driver] = i as i32;
                for &s in &net.sinks {
                    inst.pins[s] = i as i32;
                }
            }
            for u in 0..inst.size {
                for v in 0..inst.size {
                    let a = inst.coord(u);
                    let b = inst.coord(v);
                    if a[0].abs_diff(b[0]) + a[1].abs_diff(b[1]) + a[2].abs_diff(b[2]) == 1 {
                        let weight = inst.weight(u, v);
                        inst.neighbors[u].push((v, weight));
                    }
                }
            }
            for seed in 0..20 {
                let mut routes = vec![
                    Some(Route {
                        vertices: vec![0, 1, 4, 5],
                        edges: vec![(0, 1), (1, 4), (4, 5)],
                        delay: 3,
                    }),
                    Some(Route {
                        vertices: vec![1, 2, 3, 4],
                        edges: vec![(1, 2), (1, 4), (3, 4)],
                        delay: 3,
                    }),
                ];
                let original = routes.clone();
                let mut count = vec![0; inst.size];
                for r in &routes {
                    occupy(&mut count, r.as_ref().unwrap(), true);
                }
                let before = count.clone();
                let repaired = legalize_conflicts(
                    &inst,
                    &mut Search::new(inst.size),
                    &mut routes,
                    &mut count,
                    &mut Rng(seed),
                );
                assert_eq!(repaired, layers == 2);
                let mut rebuilt = vec![0; inst.size];
                for (nid, route) in routes.iter().enumerate() {
                    let route = route.as_ref().unwrap();
                    occupy(&mut rebuilt, route, true);
                    assert_eq!(route.edges.len() + 1, route.vertices.len());
                    assert!(route
                        .vertices
                        .iter()
                        .all(|&v| inst.pins[v] < 0 || inst.pins[v] == nid as i32));
                    if !repaired {
                        assert_eq!(route.edges, original[nid].as_ref().unwrap().edges);
                    }
                }
                assert_eq!(count, rebuilt);
                if repaired {
                    assert!(count.iter().all(|&n| n <= 1));
                } else {
                    assert_eq!(count, before);
                }
            }
        }
    }
    #[test]
    fn moving_goal_astar_matches_full_dijkstra() {
        let mut rng = Rng(20261022);
        for _ in 0..120 {
            let mut inst = Instance {
                flow_pricing: 0,
                construction_weight: 1,
                fanout_priority: 0,
                slow_congestion: false,
                legalize_conflicts: 0,
                cost_distance: false,
                cost_distance_rooted: false,
                cost_distance_discount: false,
                name: "differential".into(),
                w: 4,
                h: 4,
                wh: 16,
                size: 48,
                ld: vec![1 + rng.next() % 7, 1 + rng.next() % 7, 1 + rng.next() % 7],
                via: 1 + rng.next() % 5,
                nets: vec![Net {
                    id: 0,
                    driver: 0,
                    sinks: vec![15, 32, 47],
                    cross_singleton: false,
                }],
                cross_profiles: vec![None; 48],
                pins: vec![-1; 48],
                neighbors: vec![vec![]; 48],
                free_distance: Vec::new(),
            };
            inst.pins[0] = 0;
            for &s in &inst.nets[0].sinks {
                inst.pins[s] = 0;
            }
            for u in 0..48 {
                let (x, y, z) = (u % 4, u / 4 % 4, u / 16);
                for v in [
                    if x > 0 { Some(u - 1) } else { None },
                    if x < 3 { Some(u + 1) } else { None },
                    if y > 0 { Some(u - 4) } else { None },
                    if y < 3 { Some(u + 4) } else { None },
                    if z > 0 { Some(u - 16) } else { None },
                    if z < 2 { Some(u + 16) } else { None },
                ]
                .into_iter()
                .flatten()
                {
                    let w = inst.weight(u, v);
                    inst.neighbors[u].push((v, w));
                }
            }
            let count: Vec<u32> = (0..48)
                .map(|u| {
                    if inst.pins[u] < 0 && rng.next() % 5 == 0 {
                        1
                    } else {
                        0
                    }
                })
                .collect();
            let history: Vec<u64> = (0..48).map(|_| rng.next() % 8000).collect();
            inst.fanout_priority = rng.next() % 3;
            let divisor = match inst.fanout_priority {
                0 => 1024,
                1 => (3.0f64.sqrt() * 1024.0).floor() as u64,
                _ => 3 * 1024,
            };
            let priced = |v: usize| (history[v] + count[v] as u64 * 2048) * 1024 / divisor;
            for strict in [true, false] {
                let mut d = vec![u64::MAX; 48];
                let mut prev = vec![0; 48];
                d[0] = 0;
                let mut q = BinaryHeap::from([Reverse((0, 0usize))]);
                while let Some(Reverse((du, u))) = q.pop() {
                    if d[u] != du {
                        continue;
                    }
                    for &(v, w) in &inst.neighbors[u] {
                        if strict && count[v] > 0 {
                            continue;
                        }
                        let nd = du + if strict { w } else { w * 1024 + priced(v) };
                        if nd < d[v] {
                            d[v] = nd;
                            prev[v] = u;
                            q.push(Reverse((nd, v)));
                        }
                    }
                }
                let route =
                    Search::new(48).route(&inst, 0, &count, &history, 2048, strict, rng.next());
                if inst.nets[0].sinks.iter().any(|&s| d[s] == u64::MAX) {
                    assert!(route.is_none());
                    continue;
                }
                let route = route.unwrap();
                let mut adj = vec![vec![]; 48];
                for &(a, b) in &route.edges {
                    adj[a].push(b);
                    adj[b].push(a);
                }
                let mut actual = vec![u64::MAX; 48];
                actual[0] = 0;
                let mut stack = vec![0];
                while let Some(u) = stack.pop() {
                    for &v in &adj[u] {
                        if actual[v] == u64::MAX {
                            actual[v] = actual[u]
                                + if strict {
                                    inst.weight(u, v)
                                } else {
                                    inst.weight(u, v) * 1024 + priced(v)
                                };
                            stack.push(v);
                        }
                    }
                }
                for &s in &inst.nets[0].sinks {
                    assert_eq!(actual[s], d[s]);
                }
                assert_eq!(route.vertices.len(), route.edges.len() + 1);
            }
        }
    }
    // Exhaustively enumerate every spanning tree on a 3x2 grid. Compare the
    // exact single-net SPT objective, including two sinks and blocked vertices.
    #[test]
    fn single_net_matches_exhaustive_tree_objective() {
        let mut inst = Instance {
            flow_pricing: 0,
            construction_weight: 1,
            fanout_priority: 0,
            slow_congestion: false,
            legalize_conflicts: 0,
            cost_distance: false,
            cost_distance_rooted: false,
            cost_distance_discount: false,
            name: "tiny".into(),
            w: 3,
            h: 2,
            wh: 6,
            size: 6,
            ld: vec![2],
            via: 3,
            nets: vec![Net {
                id: 9,
                driver: 0,
                sinks: vec![2, 5],
                cross_singleton: false,
            }],
            cross_profiles: vec![None; 6],
            pins: vec![0, -1, 0, -1, -1, 0],
            neighbors: vec![vec![]; 6],
            free_distance: Vec::new(),
        };
        let edges = [(0, 1), (1, 2), (3, 4), (4, 5), (0, 3), (1, 4), (2, 5)];
        for &(a, b) in &edges {
            inst.neighbors[a].push((b, 2));
            inst.neighbors[b].push((a, 2));
        }
        for block in [None, Some(1), Some(3), Some(4)] {
            let mut count = vec![0; 6];
            if let Some(b) = block {
                count[b] = 1;
            }
            let mut optimum = u64::MAX;
            for mask in 0u32..1 << edges.len() {
                let mut adj = vec![vec![]; 6];
                let mut used = vec![false; 6];
                let mut ne = 0;
                for (i, &(a, b)) in edges.iter().enumerate() {
                    if mask >> i & 1 != 0 {
                        adj[a].push(b);
                        adj[b].push(a);
                        used[a] = true;
                        used[b] = true;
                        ne += 1;
                    }
                }
                if block.map_or(false, |b| used[b]) || !used[0] || !used[2] || !used[5] {
                    continue;
                }
                let mut d = vec![u64::MAX; 6];
                d[0] = 0;
                let mut stack = vec![0];
                while let Some(u) = stack.pop() {
                    for &v in &adj[u] {
                        if d[v] == u64::MAX {
                            d[v] = d[u] + 2;
                            stack.push(v);
                        }
                    }
                }
                let nv = used.iter().filter(|&&x| x).count();
                if ne + 1 != nv || used.iter().enumerate().any(|(i, &u)| u && d[i] == u64::MAX) {
                    continue;
                }
                optimum = optimum.min(d[2] + d[5]);
            }
            for salt in 0..20 {
                let route = Search::new(6)
                    .route(&inst, 0, &count, &vec![0; 6], 0, true, salt)
                    .unwrap();
                assert_eq!(route.delay, optimum);
                assert_eq!(route.vertices.len(), route.edges.len() + 1);
            }
            if let Some(b) = block {
                inst.pins[b] = 1;
            }
            for variant in [Some(0), Some(50), Some(100), None] {
                for salt in 0..20 {
                    let route = if let Some(root_weight) = variant {
                        Search::new(6).route_tree(
                            &inst,
                            0,
                            &count,
                            &vec![0; 6],
                            0,
                            salt,
                            root_weight,
                        )
                    } else {
                        inst.cost_distance_rooted = salt % 2 == 0;
                        inst.cost_distance_discount = salt % 3 == 0;
                        cost_distance::route(
                            &inst,
                            &mut Search::new(6),
                            0,
                            &count,
                            &vec![0; 6],
                            0,
                            salt,
                        )
                    }
                    .unwrap();
                    assert_eq!(route.vertices.len(), route.edges.len() + 1);
                    assert!(block.map_or(true, |b| !route.vertices.contains(&b)));
                    let mut adj = vec![vec![]; 6];
                    for &(u, v) in &route.edges {
                        adj[u].push(v);
                        adj[v].push(u);
                    }
                    let mut distances = vec![u64::MAX; 6];
                    distances[0] = 0;
                    let mut stack = vec![0];
                    while let Some(u) = stack.pop() {
                        for &v in &adj[u] {
                            if distances[v] == u64::MAX {
                                distances[v] = distances[u] + inst.weight(u, v);
                                stack.push(v);
                            }
                        }
                    }
                    assert!(route.vertices.iter().all(|&v| distances[v] != u64::MAX));
                    assert_eq!(route.delay, distances[2] + distances[5]);
                    if variant == Some(100) {
                        assert_eq!(route.delay, optimum);
                    }
                }
            }
            if let Some(b) = block {
                inst.pins[b] = -1;
            }
            // Extra negotiation must preserve a complete legal incumbent even
            // when subsequent attempts use temporary resource overlaps.
            for root_weight in [None, Some(0), Some(50)] {
                let original = build(
                    &inst,
                    &mut Search::new(6),
                    &mut Rng(7),
                    20,
                    root_weight,
                    0,
                    0,
                )
                .unwrap();
                let improved = build(
                    &inst,
                    &mut Search::new(6),
                    &mut Rng(7),
                    20,
                    root_weight,
                    32,
                    0,
                )
                .unwrap();
                assert!(improved[0].delay <= original[0].delay);
                assert_eq!(improved[0].vertices.len(), improved[0].edges.len() + 1);
                assert!(inst.nets[0]
                    .sinks
                    .iter()
                    .all(|s| improved[0].vertices.contains(s)));
            }
        }
    }
}
