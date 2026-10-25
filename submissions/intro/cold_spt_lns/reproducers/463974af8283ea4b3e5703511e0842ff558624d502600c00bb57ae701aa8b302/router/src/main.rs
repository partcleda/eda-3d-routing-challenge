//! Independent cold construction from instance pins, followed by fixed-work LNS.
//! No route input, reference solution, learned weights, or embedded route data.
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::fs;
mod cost_distance;
#[allow(dead_code)]
mod refine;

#[derive(Clone)]
struct Net {
    id: u64,
    driver: usize,
    sinks: Vec<usize>,
}
struct Instance {
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
        for p in v["pins"].as_array().unwrap() {
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
            .map(|n| Net {
                id: n["id"].as_u64().unwrap(),
                driver: ids[&n["driver"].as_u64().unwrap()],
                sinks: n["sinks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| ids[&s.as_u64().unwrap()])
                    .collect(),
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
impl Search {
    fn new(size: usize) -> Self {
        Self {
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
                    inst.congestion_price(nid, history[v] + present * count[v] as u64)
                };
                let step = if strict { w } else { w * 1024 + extra };
                let nd = d + step;
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
fn occupy(count: &mut [u32], r: &Route, add: bool) {
    for &v in &r.vertices {
        if add {
            count[v] += 1;
        } else {
            count[v] -= 1;
        }
    }
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
            if let Some(ref r) = routes[i] {
                occupy(&mut count, r, false);
            }
            let salt = rng.next();
            let r = if inst.cost_distance {
                cost_distance::route(inst, search, i, &count, &history, present, salt)?
            } else if let Some(weight) = tree_root_weight {
                search.route_tree(inst, i, &count, &history, present, salt, weight)?
            } else {
                search.route(inst, i, &count, &history, present, false, salt)?
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
    best
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

fn refine_output(
    instance: &str,
    output: &str,
    seed: u64,
    trials: u64,
    mode: &str,
    options: &refine::Options,
) -> u64 {
    assert!(trials > 0);
    let before = if mode == "portfolio" || mode == "portfolio-joint" || mode == "portfolio-cbs" {
        let first = trials / 2;
        if first > 0 {
            let stage = if mode == "portfolio-cbs" {
                "cbs-mixed"
            } else if mode == "portfolio-joint" {
                "mixed-joint"
            } else {
                "mixed"
            };
            refine::refine_cold(instance, output, output, seed, first, stage, options);
        }
        refine::refine_cold(
            instance,
            output,
            output,
            seed.wrapping_add(1),
            trials - first,
            "chain-fast",
            options,
        )
    } else {
        refine::refine_cold(instance, output, output, seed, trials, mode, options)
    };
    if options.post_cbs_trials > 0 {
        let after = refine::refine_cold(
            instance,
            output,
            output,
            seed.wrapping_add(2),
            options.post_cbs_trials,
            "cbs-mixed",
            options,
        );
        assert!(
            after <= before,
            "post-conflict search must retain its own incumbent"
        );
        after
    } else {
        before
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: cold-spt-lns INSTANCE OUTPUT [--seed N --starts N --rounds N --sweeps N --moves N]");
        std::process::exit(2);
    }
    let (mut seed, mut starts, mut rounds, mut sweeps, mut moves) =
        (20261022u64, 2usize, 200usize, 6usize, 128usize);
    let (mut refine_trials, mut refine_mode, mut construction_weight) =
        (0u64, String::from("chain-fast"), 1u64);
    let mut tree_root_weight = None;
    let mut negotiation_rounds = 0;
    let mut fanout_priority = 0;
    let mut slow_congestion = false;
    let mut legalize_limit = 0;
    let mut use_cost_distance = false;
    let mut refine_each_start = false;
    let mut cost_distance_rooted = false;
    let mut cost_distance_discount = false;
    let mut refine_options = refine::Options::default();
    let mut emit_global_pool = false;
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
            "--post-cbs-trials" => refine_options.post_cbs_trials = a[1].parse().unwrap(),
            "--cbs-nodes" => refine_options.cbs_nodes = a[1].parse().unwrap(),
            "--joint-neutral" => refine_options.joint_neutral = a[1].parse().unwrap(),
            "--penalized-astar" => {
                refine_options.penalized_astar = a[1].parse::<u64>().unwrap() != 0
            }
            "--emit-global-pool" => emit_global_pool = a[1].parse::<u64>().unwrap() != 0,
            "--compact-shortest-tree" => {
                refine_options.compact_shortest_tree = a[1].parse::<u64>().unwrap() != 0
            }
            "--construction-weight" => construction_weight = a[1].parse().unwrap(),
            "--tree-root-weight" => tree_root_weight = Some(a[1].parse::<u64>().unwrap()),
            "--negotiation-rounds" => negotiation_rounds = a[1].parse::<usize>().unwrap(),
            "--fanout-priority" => fanout_priority = a[1].parse::<u64>().unwrap(),
            "--slow-congestion" => slow_congestion = a[1].parse::<u64>().unwrap() != 0,
            "--legalize-conflicts" => legalize_limit = a[1].parse::<usize>().unwrap(),
            "--cost-distance" => use_cost_distance = a[1].parse::<u64>().unwrap() != 0,
            "--refine-each-start" => refine_each_start = a[1].parse::<u64>().unwrap() != 0,
            "--cost-distance-rooted" => cost_distance_rooted = a[1].parse::<u64>().unwrap() != 0,
            "--cost-distance-discount" => {
                cost_distance_discount = a[1].parse::<u64>().unwrap() != 0
            }
            _ => panic!("unknown argument {}", a[0]),
        }
    }
    assert!(starts > 0 && rounds > 0);
    assert!(construction_weight >= 1);
    let mut inst = Instance::load(&args[1]);
    inst.construction_weight = construction_weight;
    assert!(fanout_priority <= 2);
    inst.fanout_priority = fanout_priority;
    inst.slow_congestion = slow_congestion;
    inst.legalize_conflicts = legalize_limit;
    inst.cost_distance = use_cost_distance;
    inst.cost_distance_rooted = cost_distance_rooted;
    inst.cost_distance_discount = cost_distance_discount;
    let mut search = Search::new(inst.size);
    let mut rng = Rng(seed);
    let mut best: Option<Vec<Route>> = None;
    let mut best_final = None;
    let mut best_final_score = u64::MAX;
    for start in 0..starts {
        eprintln!("cold start {} of {}", start + 1, starts);
        let candidate = build(
            &inst,
            &mut search,
            &mut rng,
            rounds,
            tree_root_weight,
            negotiation_rounds,
        );
        let candidate = if candidate.is_none()
            && (inst.slow_congestion || inst.cost_distance || inst.fanout_priority != 0)
        {
            eprintln!("construction experimental attempt exhausted; fresh uniform SPT fallback");
            let settings = (
                inst.slow_congestion,
                inst.fanout_priority,
                inst.construction_weight,
                inst.cost_distance,
            );
            inst.slow_congestion = false;
            inst.fanout_priority = 0;
            inst.construction_weight = 2;
            inst.cost_distance = false;
            let fallback = build(&inst, &mut search, &mut rng, 1000, None, negotiation_rounds);
            (
                inst.slow_congestion,
                inst.fanout_priority,
                inst.construction_weight,
                inst.cost_distance,
            ) = settings;
            fallback
        } else {
            candidate
        };
        if let Some(mut routes) = candidate {
            if sweeps > 0 || moves > 0 {
                improve(&inst, &mut search, &mut routes, &mut rng, sweeps, moves);
            }
            let score: u64 = routes.iter().map(|r| r.delay).sum();
            eprintln!("start {} delay {}", start + 1, score);
            if refine_each_start && refine_trials > 0 {
                // Every basin starts with newly generated geometry; all its
                // refinement work remains inside this one timed invocation.
                fs::write(&args[2], serialize_routes(&inst, &routes))
                    .expect("write fresh cold output");
                let start_seed = seed.wrapping_add((start as u64).wrapping_mul(0x9E3779B97F4A7C15));
                let final_score = refine_output(
                    &args[1],
                    &args[2],
                    start_seed,
                    refine_trials,
                    &refine_mode,
                    &refine_options,
                );
                eprintln!(
                    "completed cold start {} final delay {}",
                    start + 1,
                    final_score
                );
                if final_score < best_final_score {
                    best_final_score = final_score;
                    best_final =
                        Some(fs::read(&args[2]).expect("read this invocation final output"));
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
    if let Some(bytes) = best_final {
        fs::write(&args[2], bytes).expect("write best internally generated final output");
        println!("total_delay={best_final_score}");
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
    if emit_global_pool {
        refine::write_pool(&args[1], &args[2], &format!("{}.pool.json", args[2]), seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_repair_resolves_crossings_or_restores_the_incumbent() {
        for layers in [1, 2] {
            let mut inst = Instance {
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
                    },
                    Net {
                        id: 1,
                        driver: 2,
                        sinks: vec![3],
                    },
                ],
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
                }],
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
            }],
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
                let original =
                    build(&inst, &mut Search::new(6), &mut Rng(7), 20, root_weight, 0).unwrap();
                let improved =
                    build(&inst, &mut Search::new(6), &mut Rng(7), 20, root_weight, 32).unwrap();
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
