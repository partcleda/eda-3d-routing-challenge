//! Exact two-parent recombination over complete layouts from this invocation.
//!
//! For net i, binary x_i chooses parent B when set and parent A otherwise.
//! Cross-parent vertex collisions are implications. Minimum-weight closure
//! therefore finds the minimum physical sink-delay assignment over this pair.
//! This is exact only for the two supplied boards.
use super::{Instance, Route};
use std::collections::{BTreeSet, VecDeque};

const MAX_NETS: usize = 2048;
const MAX_GRID_VERTICES: usize = 2_000_000;
const MAX_IMPLICATIONS: usize = 250_000;
const MAX_RESIDUAL_EDGE_SCANS: u64 = 10_000_000;
pub(super) const MAX_STREAM_PARENTS: usize = 8;
pub(super) const MAX_STREAM_TOTAL_RESIDUAL_EDGE_SCANS: u64 =
    (MAX_STREAM_PARENTS as u64 - 1) * MAX_RESIDUAL_EDGE_SCANS;

#[derive(Debug)]
pub(super) enum Attempt {
    Improved { routes: Vec<Route>, score: u64, parent_a: u64, parent_b: u64, implications: usize, scans: u64, changed_nets: usize },
    NoImprovement { parent_a: u64, parent_b: u64, score: u64, implications: usize, scans: u64 },
    Refused(&'static str),
}

pub(super) struct StreamState {
    pub(super) accumulator: Option<Vec<Route>>,
    pub(super) score: u64,
    pub(super) parents: usize,
    pub(super) residual_edge_scans: u64,
    pub(super) stopped: bool,
    pub(super) refused: bool,
}

impl Default for StreamState {
    fn default() -> Self {
        Self { accumulator: None, score: u64::MAX, parents: 0, residual_edge_scans: 0, stopped: false, refused: false }
    }
}

#[derive(Debug)]
pub(super) enum StreamPush {
    Started { score: u64 },
    Improved { parent_a: u64, parent_b: u64, score: u64, implications: usize, scans: u64, changed_nets: usize },
    AdoptedIncoming { parent_a: u64, parent_b: u64, score: u64, implications: usize, scans: u64 },
    NoImprovement { parent_a: u64, parent_b: u64, score: u64, implications: usize, scans: u64 },
    Refused(&'static str),
    Ignored,
}

pub(super) fn stream_push_parent(inst: &Instance, state: &mut StreamState, incoming: &[Route]) -> StreamPush {
    if state.stopped || state.parents >= MAX_STREAM_PARENTS { return StreamPush::Ignored; }
    if !within_caps(inst) { state.stopped = true; state.refused = true; return StreamPush::Refused("parent_dimensions_exceed_caps"); }
    if let Some(current) = state.accumulator.as_ref() {
        let remaining = match MAX_STREAM_TOTAL_RESIDUAL_EDGE_SCANS.checked_sub(state.residual_edge_scans) {
            Some(value) if value > 0 => value,
            _ => { state.stopped = true; state.refused = true; return StreamPush::Refused("invocation_scan_cap"); }
        };
        match combine_two_layouts_with_scan_limit(inst, current, incoming, remaining) {
            Attempt::Improved { routes, score, parent_a, parent_b, implications, scans, changed_nets } => {
                let Some(total_scans) = state.residual_edge_scans.checked_add(scans) else {
                    state.stopped = true;
                    state.refused = true;
                    return StreamPush::Refused("scan_counter_overflow");
                };
                state.residual_edge_scans = total_scans;
                state.accumulator = Some(routes);
                state.score = score;
                state.parents += 1;
                if state.parents >= MAX_STREAM_PARENTS { state.stopped = true; }
                StreamPush::Improved { parent_a, parent_b, score, implications, scans, changed_nets }
            }
            Attempt::NoImprovement { parent_a, parent_b, score, implications, scans } => {
                let Some(total_scans) = state.residual_edge_scans.checked_add(scans) else {
                    state.stopped = true;
                    state.refused = true;
                    return StreamPush::Refused("scan_counter_overflow");
                };
                state.residual_edge_scans = total_scans;
                if parent_a != state.score {
                    state.stopped = true;
                    state.refused = true;
                    return StreamPush::Refused("accumulator_score_mismatch");
                }
                // The exact pair selector can return its best input without a
                // strict mixed-layout gain. Keep whichever complete parent is
                // cheaper so the running accumulator never gets worse merely
                // because an incoming parent improved every net together.
                if parent_b < parent_a {
                    let Some(cloned) = try_clone_layout(incoming) else {
                        state.stopped = true;
                        state.refused = true;
                        return StreamPush::Refused("incoming_parent_clone_allocation");
                    };
                    let Some(scored) = score_serialized_routes(inst, cloned) else {
                        state.stopped = true;
                        state.refused = true;
                        return StreamPush::Refused("incoming_parent_validation");
                    };
                    let Some(actual_score) = scored.iter().try_fold(0u64, |sum, route| sum.checked_add(route.delay)) else {
                        state.stopped = true;
                        state.refused = true;
                        return StreamPush::Refused("incoming_parent_score_overflow");
                    };
                    if actual_score != parent_b || score != parent_b {
                        state.stopped = true;
                        state.refused = true;
                        return StreamPush::Refused("incoming_parent_score_mismatch");
                    }
                    state.accumulator = Some(scored);
                    state.score = actual_score;
                    state.parents += 1;
                    if state.parents >= MAX_STREAM_PARENTS { state.stopped = true; }
                    StreamPush::AdoptedIncoming { parent_a, parent_b, score: actual_score, implications, scans }
                } else {
                    if score != parent_a {
                        state.stopped = true;
                        state.refused = true;
                        return StreamPush::Refused("accumulator_pair_score_mismatch");
                    }
                    state.score = parent_a;
                    state.parents += 1;
                    if state.parents >= MAX_STREAM_PARENTS { state.stopped = true; }
                    StreamPush::NoImprovement { parent_a, parent_b, score: parent_a, implications, scans }
                }
            }
            Attempt::Refused(reason) => { state.stopped = true; state.refused = true; StreamPush::Refused(reason) }
        }
    } else {
        let Some(cloned) = try_clone_layout(incoming) else { state.stopped = true; state.refused = true; return StreamPush::Refused("first_parent_clone_allocation"); };
        let Some(scored) = score_serialized_routes(inst, cloned) else { state.stopped = true; state.refused = true; return StreamPush::Refused("first_parent_invalid"); };
        let Some(score) = scored.iter().try_fold(0u64, |sum, route| sum.checked_add(route.delay)) else {
            state.stopped = true;
            state.refused = true;
            return StreamPush::Refused("first_parent_score_overflow");
        };
        state.accumulator = Some(scored);
        state.score = score;
        state.parents = 1;
        StreamPush::Started { score }
    }
}


pub(super) fn within_caps(inst: &Instance) -> bool {
    inst.nets.len() > 0 && inst.nets.len() <= MAX_NETS && inst.size <= MAX_GRID_VERTICES
}

fn try_clone_route(route: &Route) -> Option<Route> {
    let mut vertices = Vec::new(); vertices.try_reserve_exact(route.vertices.len()).ok()?;
    vertices.extend_from_slice(&route.vertices);
    let mut edges = Vec::new(); edges.try_reserve_exact(route.edges.len()).ok()?;
    edges.extend_from_slice(&route.edges);
    Some(Route { vertices, edges, delay: route.delay })
}

pub(super) fn try_clone_layout(routes: &[Route]) -> Option<Vec<Route>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(routes.len()).ok()?;
    for route in routes { copy.push(try_clone_route(route)?); }
    Some(copy)
}

#[derive(Clone)]
struct Edge { to: usize, rev: usize, cap: u128 }
struct Dinic { graph: Vec<Vec<Edge>>, level: Vec<i32>, next: Vec<usize>, scans: u64, scan_limit: u64 }
impl Dinic {
    fn new(nodes: usize, scan_limit: u64) -> Option<Self> {
        let mut graph = Vec::new();
        graph.try_reserve_exact(nodes).ok()?;
        graph.resize_with(nodes, Vec::new);
        let mut level = Vec::new(); level.try_reserve_exact(nodes).ok()?; level.resize(nodes, -1);
        let mut next = Vec::new(); next.try_reserve_exact(nodes).ok()?; next.resize(nodes, 0);
        Some(Self { graph, level, next, scans: 0, scan_limit })
    }
    fn add_edge(&mut self, from: usize, to: usize, cap: u128) -> Option<()> {
        let from_rev = self.graph[to].len();
        let to_rev = self.graph[from].len();
        self.graph[from].try_reserve(1).ok()?;
        self.graph[to].try_reserve(1).ok()?;
        self.graph[from].push(Edge { to, rev: from_rev, cap });
        self.graph[to].push(Edge { to: from, rev: to_rev, cap: 0 });
        Some(())
    }
    fn charge(&mut self) -> Option<()> {
        if self.scans >= self.scan_limit { return None; }
        self.scans += 1;
        Some(())
    }
    fn make_levels(&mut self, source: usize, sink: usize) -> Option<bool> {
        self.level.fill(-1);
        self.level[source] = 0;
        let mut queue = VecDeque::new();
        queue.try_reserve(self.graph.len()).ok()?;
        queue.push_back(source);
        while let Some(v) = queue.pop_front() {
            for i in 0..self.graph[v].len() {
                self.charge()?;
                let edge = &self.graph[v][i];
                if edge.cap > 0 && self.level[edge.to] < 0 {
                    self.level[edge.to] = self.level[v] + 1;
                    queue.push_back(edge.to);
                }
            }
        }
        Some(self.level[sink] >= 0)
    }
    fn send(&mut self, v: usize, sink: usize, amount: u128) -> Option<u128> {
        if v == sink { return Some(amount); }
        while self.next[v] < self.graph[v].len() {
            let i = self.next[v];
            self.charge()?;
            let (to, rev, cap) = { let e = &self.graph[v][i]; (e.to, e.rev, e.cap) };
            if cap > 0 && self.level[to] == self.level[v] + 1 {
                let pushed = self.send(to, sink, amount.min(cap))?;
                if pushed > 0 {
                    self.graph[v][i].cap = cap.checked_sub(pushed)?;
                    self.graph[to][rev].cap = self.graph[to][rev].cap.checked_add(pushed)?;
                    return Some(pushed);
                }
            }
            self.next[v] += 1;
        }
        Some(0)
    }
    fn max_flow(&mut self, source: usize, sink: usize) -> Option<u128> {
        let mut total = 0u128;
        while self.make_levels(source, sink)? {
            self.next.fill(0);
            loop {
                let pushed = self.send(source, sink, u128::MAX)?;
                if pushed == 0 { break; }
                total = total.checked_add(pushed)?;
            }
        }
        Some(total)
    }
    fn source_side(&mut self, source: usize) -> Option<Vec<bool>> {
        let mut seen = vec![false; self.graph.len()];
        seen[source] = true;
        let mut queue = VecDeque::new();
        queue.try_reserve(self.graph.len()).ok()?;
        queue.push_back(source);
        while let Some(v) = queue.pop_front() {
            for i in 0..self.graph[v].len() {
                self.charge()?;
                let e = &self.graph[v][i];
                if e.cap > 0 && !seen[e.to] { seen[e.to] = true; queue.push_back(e.to); }
            }
        }
        Some(seen)
    }
}

struct BoardCost { total: u64, per_net: Vec<u64> }

fn board_delay_checked(inst: &Instance, routes: &[Route], check_recorded_delay: bool) -> Option<BoardCost> {
    let n = inst.nets.len();
    if n == 0 || n > MAX_NETS || routes.len() != n || inst.size > MAX_GRID_VERTICES
        || inst.pins.len() != inst.size || inst.neighbors.len() != inst.size { return None; }
    // Parent layouts are positional over this exact net vector. Validate that
    // the vector itself has unique identities and that the pin-owner map is a
    // complete encoding of its declared terminals before using indices in the
    // implication graph.
    let mut net_ids = BTreeSet::new();
    let mut terminal_owner = Vec::new();
    terminal_owner.try_reserve_exact(inst.size).ok()?;
    terminal_owner.resize(inst.size, usize::MAX);
    for (i, net) in inst.nets.iter().enumerate() {
        if !net_ids.insert(net.id) || net.driver >= inst.size { return None; }
        for &terminal in std::iter::once(&net.driver).chain(net.sinks.iter()) {
            if terminal >= inst.size { return None; }
            if terminal_owner[terminal] != usize::MAX && terminal_owner[terminal] != i { return None; }
            terminal_owner[terminal] = i;
        }
    }
    for (v, &owner) in inst.pins.iter().enumerate() {
        match owner {
            -1 if terminal_owner[v] == usize::MAX => {},
            x if x >= 0 && (x as usize) < n && terminal_owner[v] == x as usize => {},
            _ => return None,
        }
    }

    let mut owners = Vec::new(); owners.try_reserve_exact(inst.size).ok()?; owners.resize(inst.size, usize::MAX);
    let mut adjacency = Vec::new(); adjacency.try_reserve_exact(inst.size).ok()?; adjacency.resize_with(inst.size, Vec::new);
    for (i, route) in routes.iter().enumerate() {
        if route.vertices.is_empty() { return None; }
        let mut vertices = BTreeSet::new();
        for &v in &route.vertices {
            if v >= inst.size || !vertices.insert(v) || owners[v] != usize::MAX { return None; }
            if inst.pins[v] >= 0 && inst.pins[v] as usize != i { return None; }
            owners[v] = i;
        }
        let net = &inst.nets[i];
        if !vertices.contains(&net.driver) || net.sinks.iter().any(|s| !vertices.contains(s)) { return None; }
        let mut edge_set = BTreeSet::new();
        for &(a, b) in &route.edges {
            if a >= inst.size || b >= inst.size || a == b || !vertices.contains(&a) || !vertices.contains(&b) { return None; }
            let key = (a.min(b), a.max(b));
            if !edge_set.insert(key) { return None; }
            let weight = inst.weight(a, b);
            if !inst.neighbors[a].iter().any(|&(v, w)| v == b && w == weight)
                || !inst.neighbors[b].iter().any(|&(v, w)| v == a && w == weight) { return None; }
            adjacency[a].push((b, weight));
            adjacency[b].push((a, weight));
        }
        if route.edges.len().checked_add(1)? != vertices.len() { return None; }
    }
    let mut total = 0u64;
    let mut per_net = Vec::new(); per_net.try_reserve_exact(n).ok()?;
    let mut distance = vec![u64::MAX; inst.size];
    let mut queue = Vec::new();
    for (i, route) in routes.iter().enumerate() {
        queue.clear(); queue.try_reserve(route.vertices.len()).ok()?;
        distance[inst.nets[i].driver] = 0;
        queue.push(inst.nets[i].driver);
        let mut head = 0;
        while head < queue.len() {
            let u = queue[head]; head += 1;
            for &(v, weight) in &adjacency[u] {
                if distance[v] == u64::MAX {
                    distance[v] = distance[u].checked_add(weight)?;
                    queue.push(v);
                }
            }
        }
        if route.vertices.iter().any(|&v| distance[v] == u64::MAX) { return None; }
        let mut net_delay = 0u64;
        for &sink in &inst.nets[i].sinks {
            net_delay = net_delay.checked_add(distance[sink])?;
        }
        if check_recorded_delay && net_delay != route.delay { return None; }
        total = total.checked_add(net_delay)?;
        per_net.push(net_delay);
        for &v in &route.vertices { distance[v] = u64::MAX; }
    }
    Some(BoardCost { total, per_net })
}

fn board_delay(inst: &Instance, routes: &[Route]) -> Option<BoardCost> {
    board_delay_checked(inst, routes, true)
}

pub(super) fn score_serialized_routes(inst: &Instance, mut routes: Vec<Route>) -> Option<Vec<Route>> {
    let costs = board_delay_checked(inst, &routes, false)?;
    if costs.per_net.len() != routes.len() { return None; }
    for (route, delay) in routes.iter_mut().zip(costs.per_net) { route.delay = delay; }
    board_delay(inst, &routes)?;
    Some(routes)
}

fn implication_pairs(inst: &Instance, a: &[Route], b: &[Route]) -> Option<BTreeSet<(usize, usize)>> {
    let mut owner_a = Vec::new(); owner_a.try_reserve_exact(inst.size).ok()?; owner_a.resize(inst.size, usize::MAX);
    let mut owner_b = Vec::new(); owner_b.try_reserve_exact(inst.size).ok()?; owner_b.resize(inst.size, usize::MAX);
    for (i, r) in a.iter().enumerate() { for &v in &r.vertices { owner_a[v] = i; } }
    for (i, r) in b.iter().enumerate() { for &v in &r.vertices { owner_b[v] = i; } }
    let mut pairs = BTreeSet::new();
    for (i, r) in b.iter().enumerate() {
        for &v in &r.vertices {
            let j = owner_a[v];
            if j != usize::MAX && i != j && pairs.insert((i, j)) && pairs.len() > MAX_IMPLICATIONS { return None; }
        }
    }
    for (i, r) in a.iter().enumerate() {
        for &v in &r.vertices {
            let j = owner_b[v];
            if j != usize::MAX && i != j && pairs.insert((j, i)) && pairs.len() > MAX_IMPLICATIONS { return None; }
        }
    }
    Some(pairs)
}

pub(super) fn combine_two_layouts(inst: &Instance, a: &[Route], b: &[Route]) -> Attempt {
    combine_two_layouts_with_scan_limit(inst, a, b, MAX_RESIDUAL_EDGE_SCANS)
}

pub(super) fn combine_two_layouts_with_scan_limit(
    inst: &Instance,
    a: &[Route],
    b: &[Route],
    scan_limit: u64,
) -> Attempt {
    let Some(board_a) = board_delay(inst, a) else {
        eprintln!("board-recombine status=refused reason=invalid_parent_a");
        return Attempt::Refused("invalid_parent_a");
    };
    let Some(board_b) = board_delay(inst, b) else {
        eprintln!("board-recombine status=refused reason=invalid_parent_b parent_a_delay={}", board_a.total);
        return Attempt::Refused("invalid_parent_b");
    };
    let score_a = board_a.total;
    let score_b = board_b.total;
    let geometry_different = a.iter().zip(b).filter(|(ra, rb)| ra.edges != rb.edges).count();
    eprintln!("board-recombine parents_valid count=2 parent_a_delay={} parent_b_delay={} nets={} geometry_different_nets={}", score_a, score_b, inst.nets.len(), geometry_different);
    let Some(pairs) = implication_pairs(inst, a, b) else { return Attempt::Refused("implication_cap_or_allocation"); };
    let n = inst.nets.len();
    let source = n; let sink = n + 1;
    let Some(mut flow) = Dinic::new(n + 2, scan_limit.min(MAX_RESIDUAL_EDGE_SCANS)) else { return Attempt::Refused("flow_allocation"); };
    let mut total_abs_delta = 0u128;
    let mut benefit = Vec::new();
    if benefit.try_reserve_exact(n).is_err() { return Attempt::Refused("benefit_allocation"); }
    for i in 0..n {
        let da = board_a.per_net[i];
        let db = board_b.per_net[i];
        let p = da as i128 - db as i128;
        let abs = p.unsigned_abs();
        let Some(sum) = total_abs_delta.checked_add(abs) else { return Attempt::Refused("capacity_overflow"); };
        total_abs_delta = sum;
        benefit.push(p);
    }
    let Some(infinite) = total_abs_delta.checked_add(1) else { return Attempt::Refused("capacity_overflow"); };
    for (i, &p) in benefit.iter().enumerate() {
        if p > 0 { if flow.add_edge(source, i, p as u128).is_none() { return Attempt::Refused("flow_allocation"); } }
        else if p < 0 { if flow.add_edge(i, sink, p.unsigned_abs()).is_none() { return Attempt::Refused("flow_allocation"); } }
    }
    for &(i, j) in &pairs { if flow.add_edge(i, j, infinite).is_none() { return Attempt::Refused("flow_allocation"); } }
    let Some(flow_value) = flow.max_flow(source, sink) else { return Attempt::Refused("flow_scan_cap_or_overflow"); };
    if flow_value > total_abs_delta { return Attempt::Refused("cut_crossed_implication_capacity"); }
    let Some(side) = flow.source_side(source) else { return Attempt::Refused("cut_scan_cap_or_allocation"); };
    if pairs.iter().any(|&(i, j)| side[i] && !side[j]) { return Attempt::Refused("cut_violates_implication"); }
    let selected_b_nets = side[..n].iter().filter(|&&selected| selected).count();
    eprintln!("board-recombine cut_complete selected_parent_b_nets={} parent_a_nets={} implications={} residual_edge_scans={}", selected_b_nets, n, pairs.len(), flow.scans);
    let mut result = Vec::new();
    if result.try_reserve_exact(n).is_err() { return Attempt::Refused("result_allocation"); }
    let mut changed = 0usize;
    for i in 0..n {
        let selected = if side[i] { &b[i] } else { &a[i] };
        let Some(route) = try_clone_route(selected) else { return Attempt::Refused("result_route_allocation"); };
        result.push(route);
        if side[i] && a[i].edges != b[i].edges { changed += 1; }
    }
    let Some(recombined) = board_delay(inst, &result) else { return Attempt::Refused("recombined_layout_failed_validation"); };
    let score = recombined.total;
    if score > score_a.min(score_b) { return Attempt::Refused("objective_regression"); }
    let scans = flow.scans;
    if score < score_a.min(score_b) {
        Attempt::Improved { routes: result, score, parent_a: score_a, parent_b: score_b, implications: pairs.len(), scans, changed_nets: changed }
    } else {
        Attempt::NoImprovement { parent_a: score_a, parent_b: score_b, score, implications: pairs.len(), scans }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Net};

    fn fixture() -> Instance {
        let w = 3usize; let h = 3usize; let wh = 9usize; let size = 9usize;
        let mut neighbors = vec![Vec::new(); size];
        for y in 0..h { for x in 0..w {
            let u=y*w+x;
            if x+1<w { neighbors[u].push((u+1,1)); neighbors[u+1].push((u,1)); }
            if y+1<h { neighbors[u].push((u+w,1)); neighbors[u+w].push((u,1)); }
        }}
        let mut pins=vec![-1;size]; pins[0]=0; pins[2]=0; pins[6]=1; pins[8]=1;
        Instance { flow_pricing:0, construction_weight:1, fanout_priority:0,
            slow_congestion:false, legalize_conflicts:0, cost_distance:false,
            cost_distance_rooted:false, cost_distance_discount:false,
            name:"authored-cross-parent-closure".into(), w,h,wh,size,ld:vec![1],via:8,
            nets:vec![Net{id:10,driver:0,sinks:vec![2],cross_singleton:false}, Net{id:20,driver:6,sinks:vec![8],cross_singleton:false}],
            cross_profiles:vec![None;2], pins, neighbors, free_distance:vec![] }
    }
    fn from_path(inst:&Instance,path:&[usize])->Route {
        let mut edges:Vec<_>=path.windows(2).map(|p|(p[0].min(p[1]),p[0].max(p[1]))).collect();
        edges.sort_unstable();
        let mut vertices=path.to_vec(); vertices.sort_unstable(); vertices.dedup();
        Route{vertices,delay:(path.len()-1) as u64,edges}
    }
    fn parents(inst:&Instance)->(Vec<Route>,Vec<Route>) {
        // A: short net 0 + long net 1 = 6. B: long net 0 + short net 1 = 6.
        // B0 and A1 overlap at (0,1),(1,1),(2,1), imposing x0 -> x1.
        let a=vec![from_path(inst,&[0,1,2]),from_path(inst,&[6,3,4,5,8])];
        let b=vec![from_path(inst,&[0,3,4,5,2]),from_path(inst,&[6,7,8])];
        (a,b)
    }
    fn streaming_fixture() -> Instance {
        let w=9usize; let h=9usize; let wh=w*h; let size=wh;
        let mut neighbors=vec![Vec::new();size];
        for y in 0..h { for x in 0..w {
            let u=y*w+x;
            if x+1<w { neighbors[u].push((u+1,1)); neighbors[u+1].push((u,1)); }
            if y+1<h { neighbors[u].push((u+w,1)); neighbors[u+w].push((u,1)); }
        }}
        let mut pins=vec![-1;size];
        let nets=vec![
            Net{id:100,driver:9,sinks:vec![11],cross_singleton:false},
            Net{id:200,driver:13,sinks:vec![15],cross_singleton:false},
            Net{id:300,driver:54,sinks:vec![56],cross_singleton:false},
        ];
        for (i,net) in nets.iter().enumerate() { pins[net.driver]=i as i32; for &sink in &net.sinks { pins[sink]=i as i32; } }
        Instance { flow_pricing:0, construction_weight:1, fanout_priority:0,
            slow_congestion:false, legalize_conflicts:0, cost_distance:false,
            cost_distance_rooted:false, cost_distance_discount:false,
            name:"authored-three-parent-stream-fold".into(), w,h,wh,size,ld:vec![1],via:8,
            nets, cross_profiles:vec![None;3], pins, neighbors, free_distance:vec![] }
    }
    fn streaming_parents(inst:&Instance)->[Vec<Route>;3] {
        let short0=from_path(inst,&[9,10,11]);
        let long0=from_path(inst,&[9,18,27,28,29,20,11]);
        let short1=from_path(inst,&[13,14,15]);
        let long1=from_path(inst,&[13,22,31,32,33,24,15]);
        let short2=from_path(inst,&[54,55,56]);
        let long2=from_path(inst,&[54,63,72,73,74,65,56]);
        [vec![short0.clone(),long1.clone(),long2.clone()],
         vec![long0.clone(),short1.clone(),long2],
         vec![long0,long1,short2]]
    }
    #[test]
    fn min_cut_recombines_two_legal_parent_boards_into_a_strictly_better_board() {
        let inst=fixture(); let (a,b)=parents(&inst);
        assert_eq!(board_delay(&inst,&a).unwrap().total,6);
        assert_eq!(board_delay(&inst,&b).unwrap().total,6);
        let mut best=u64::MAX;
        for mask in 0..4usize {
            let board:Vec<_>=(0..2).map(|i|if mask&(1<<i)!=0 { b[i].clone() } else { a[i].clone() }).collect();
            if let Some(cost)=board_delay(&inst,&board) { best=best.min(cost.total); }
        }
        match combine_two_layouts(&inst,&a,&b) {
            Attempt::Improved{routes,score,parent_a,parent_b,implications,changed_nets,..}=>{
                assert_eq!((parent_a,parent_b,score),(6,6,4));
                assert_eq!(score,best);
                assert_eq!(changed_nets,1);
                assert!(implications>0);
                assert_eq!(routes[0].edges,a[0].edges);
                assert_eq!(routes[1].edges,b[1].edges);
                assert_eq!(board_delay(&inst,&routes).unwrap().total,score);
            }
            other=>panic!("expected strict physical improvement from the exact two-parent pool, got {other:?}"),
        }
    }
    #[test]
    fn reversed_parent_order_still_finds_the_authored_mixed_assignment() {
        let inst=fixture(); let (a,b)=parents(&inst);
        match combine_two_layouts(&inst,&b,&a) {
            Attempt::Improved{routes,score,..}=>{
                assert_eq!(score,4);
                assert_eq!(routes[0].edges,a[0].edges);
                assert_eq!(routes[1].edges,b[1].edges);
            }
            other=>panic!("expected same optimum under parent reversal, got {other:?}"),
        }
    }
    #[test]
    fn sink_occurrences_are_counted_in_physical_delay_not_unique_vertices() {
        let mut inst=fixture();
        inst.nets[1].sinks=vec![8,8];
        let (mut a,mut b)=parents(&inst);
        a[1].delay=8; b[1].delay=4;
        assert_eq!(board_delay(&inst,&a).unwrap().total,10);
        assert_eq!(board_delay(&inst,&b).unwrap().total,8);
        match combine_two_layouts(&inst,&a,&b) {
            Attempt::Improved{score,parent_a,parent_b,routes,..}=>{
                assert_eq!((parent_a,parent_b,score),(10,8,6));
                assert_eq!(routes[1].edges,b[1].edges);
                assert_eq!(board_delay(&inst,&routes).unwrap().total,score);
            }
            other=>panic!("expected multiplicity-aware recombination, got {other:?}"),
        }
    }
    #[test]
    fn invalid_parent_with_foreign_pin_is_refused_before_cut() {
        let inst=fixture(); let (a,mut b)=parents(&inst);
        b[0]=from_path(&inst,&[0,3,6,7,8,5,2]); // crosses net 1 driver 6 and sink 8
        assert!(matches!(combine_two_layouts(&inst,&a,&b),Attempt::Refused("invalid_parent_b")));
    }
    #[test]
    fn identical_boards_do_not_replace_the_incumbent_on_a_tie() {
        let inst=fixture(); let (a,_)=parents(&inst);
        assert!(matches!(combine_two_layouts(&inst,&a,&a),Attempt::NoImprovement{parent_a:6,parent_b:6,score:6,..}));
    }
    #[test]
    fn duplicate_net_identity_is_rejected_before_positional_recombination() {
        let mut inst=fixture(); inst.nets[1].id=inst.nets[0].id;
        let (a,b)=parents(&inst);
        assert!(matches!(combine_two_layouts(&inst,&a,&b),Attempt::Refused("invalid_parent_a")));
    }
    #[test]
    fn incomplete_or_misowned_pin_map_is_rejected() {
        let (a,b)=parents(&fixture());
        let mut orphan=fixture(); orphan.pins[1]=0;
        assert!(matches!(combine_two_layouts(&orphan,&a,&b),Attempt::Refused("invalid_parent_a")));
        let mut wrong_owner=fixture(); wrong_owner.pins[2]=1;
        assert!(matches!(combine_two_layouts(&wrong_owner,&a,&b),Attempt::Refused("invalid_parent_a")));
    }

    #[test]
    fn streaming_three_completed_parents_accumulates_a_route_absent_from_first_pair() {
        let inst=streaming_fixture(); let parents=streaming_parents(&inst);
        assert_eq!(board_delay(&inst,&parents[0]).unwrap().total,14);
        assert_eq!(board_delay(&inst,&parents[1]).unwrap().total,14);
        assert_eq!(board_delay(&inst,&parents[2]).unwrap().total,14);
        match combine_two_layouts(&inst,&parents[0],&parents[1]) {
            Attempt::Improved { score, routes, .. } => {
                assert_eq!(score,10);
                assert_eq!(routes[0].edges,parents[0][0].edges);
                assert_eq!(routes[1].edges,parents[1][1].edges);
                assert_eq!(routes[2].edges,parents[0][2].edges);
            }
            other=>panic!("expected first two-parent minimum, got {other:?}"),
        }
        let mut state=StreamState::default();
        assert!(matches!(stream_push_parent(&inst,&mut state,&parents[0]),StreamPush::Started{score:14}));
        assert!(matches!(stream_push_parent(&inst,&mut state,&parents[1]),StreamPush::Improved{score:10,..}));
        assert!(matches!(stream_push_parent(&inst,&mut state,&parents[2]),StreamPush::Improved{score:6,..}));
        assert_eq!((state.parents,state.score),(3,6));
        let result=state.accumulator.unwrap();
        assert_eq!(result.iter().map(|r|r.delay).sum::<u64>(),6);
        assert!(result.iter().zip(&parents[0]).any(|(got,old)|got.edges!=old.edges));
        assert_eq!(board_delay(&inst,&result).unwrap().total,6);
    }

    #[test]
    fn stream_ties_keep_the_accumulator_and_parent_cap_stops_at_eight() {
        let inst=fixture(); let (a,_)=parents(&inst);
        let mut state=StreamState::default();
        assert!(matches!(stream_push_parent(&inst,&mut state,&a),StreamPush::Started{..}));
        for _ in 1..MAX_STREAM_PARENTS {
            assert!(matches!(stream_push_parent(&inst,&mut state,&a),StreamPush::NoImprovement{..}));
            let current=state.accumulator.as_ref().unwrap();
            assert_eq!(board_delay(&inst,current).unwrap().total,state.score);
        }
        assert_eq!(state.accumulator.as_ref().unwrap()[0].edges,a[0].edges);
        assert_eq!(state.parents,8);
        assert!(state.stopped);
        assert!(matches!(stream_push_parent(&inst,&mut state,&a),StreamPush::Ignored));
    }

    #[test]
    fn stream_adopts_cheaper_incoming_when_pair_selector_returns_that_parent() {
        let inst=streaming_fixture();
        let parents=streaming_parents(&inst);
        let all_long=vec![parents[1][0].clone(),parents[0][1].clone(),parents[0][2].clone()];
        let all_short=vec![parents[0][0].clone(),parents[1][1].clone(),parents[2][2].clone()];
        assert_eq!(board_delay(&inst,&all_long).unwrap().total,18);
        assert_eq!(board_delay(&inst,&all_short).unwrap().total,6);
        let mut state=StreamState::default();
        assert!(matches!(stream_push_parent(&inst,&mut state,&all_long),StreamPush::Started{score:18}));
        assert_eq!(board_delay(&inst,state.accumulator.as_ref().unwrap()).unwrap().total,state.score);
        match stream_push_parent(&inst,&mut state,&all_short) {
            StreamPush::AdoptedIncoming{parent_a:18,parent_b:6,score:6,..}=>{},
            other=>panic!("expected incoming-parent adoption when the pair result ties its best parent, got {other:?}"),
        }
        assert_eq!(state.score,6);
        assert_eq!(state.accumulator.as_ref().unwrap().iter().map(|r|r.delay).sum::<u64>(),6);
        assert_eq!(board_delay(&inst,state.accumulator.as_ref().unwrap()).unwrap().total,state.score);
        assert_eq!(state.accumulator.as_ref().unwrap().iter().map(|r|&r.edges).collect::<Vec<_>>(),all_short.iter().map(|r|&r.edges).collect::<Vec<_>>());
    }

    #[test]
    fn stream_keeps_better_accumulator_when_incoming_parent_is_worse() {
        let inst=streaming_fixture();
        let parents=streaming_parents(&inst);
        let all_long=vec![parents[1][0].clone(),parents[0][1].clone(),parents[0][2].clone()];
        let all_short=vec![parents[0][0].clone(),parents[1][1].clone(),parents[2][2].clone()];
        let mut state=StreamState::default();
        assert!(matches!(stream_push_parent(&inst,&mut state,&all_short),StreamPush::Started{score:6}));
        let before=state.accumulator.as_ref().unwrap().iter().map(|r|r.edges.clone()).collect::<Vec<_>>();
        match stream_push_parent(&inst,&mut state,&all_long) {
            StreamPush::NoImprovement{parent_a:6,parent_b:18,score:6,..}=>{},
            other=>panic!("expected the better accumulator to survive a worse incoming parent, got {other:?}"),
        }
        assert_eq!(state.score,6);
        assert_eq!(state.accumulator.as_ref().unwrap().iter().map(|r|r.edges.clone()).collect::<Vec<_>>(),before);
        assert_eq!(board_delay(&inst,state.accumulator.as_ref().unwrap()).unwrap().total,state.score);
    }

    #[test]
    fn stream_illegal_parent_overflow_and_exhausted_pair_budget_fail_closed() {
        let inst=fixture(); let (a,mut b)=parents(&inst);
        b[0]=from_path(&inst,&[0,3,6,7,8,5,2]);
        let mut state=StreamState::default();
        assert!(matches!(stream_push_parent(&inst,&mut state,&a),StreamPush::Started{..}));
        assert!(matches!(stream_push_parent(&inst,&mut state,&b),StreamPush::Refused("invalid_parent_b")));
        assert_eq!(state.accumulator.as_ref().unwrap()[0].edges,a[0].edges);

        let cap_inst=fixture(); let (cap_a,cap_b)=parents(&cap_inst);
        assert!(matches!(combine_two_layouts_with_scan_limit(&cap_inst,&cap_a,&cap_b,0),Attempt::Refused("flow_scan_cap_or_overflow")));

        let mut scan_state=StreamState::default();
        assert!(matches!(stream_push_parent(&cap_inst,&mut scan_state,&cap_a),StreamPush::Started{..}));
        scan_state.residual_edge_scans=MAX_STREAM_TOTAL_RESIDUAL_EDGE_SCANS;
        assert!(matches!(stream_push_parent(&cap_inst,&mut scan_state,&cap_b),StreamPush::Refused("invocation_scan_cap")));
        assert_eq!(scan_state.accumulator.as_ref().unwrap()[0].edges,cap_a[0].edges);

        let mut overflow=streaming_fixture();
        overflow.ld[0]=u64::MAX;
        for list in &mut overflow.neighbors { for edge in list { edge.1=u64::MAX; } }
        let huge=streaming_parents(&overflow);
        assert!(matches!(combine_two_layouts(&overflow,&huge[0],&huge[1]),Attempt::Refused("invalid_parent_a")));
        let mut overflow_state=StreamState::default();
        assert!(matches!(stream_push_parent(&overflow,&mut overflow_state,&huge[0]),StreamPush::Refused("first_parent_invalid")));
        assert!(overflow_state.accumulator.is_none());
    }
}
