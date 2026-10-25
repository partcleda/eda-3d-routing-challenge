//! Exact full-graph tree pricing for small-fanout raw construction.
//!
//! The primary objective is 1024 * sum_s dist(root,s) + sum_{v in T} price[v].
//! Reconstruction follows the DP backpointer union with a positive-physical
//! shortest-path tree, retains root-to-sink paths, and verifies objective equality.

use crate::{Instance, Route};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

const INF: u64 = u64::MAX / 8;
const NONE: u32 = u32::MAX;
const SEED: u32 = u32::MAX - 1;
const MOVE_TAG: u32 = 0x8000_0000;
const MERGE_TAG: u32 = 0x4000_0000;
const MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;

fn add(a: u64, b: u64) -> Option<u64> {
    a.checked_add(b).filter(|&x| x < INF)
}

#[derive(Clone, Debug)]
struct Solution {
    vertices: Vec<usize>,
    edges: Vec<(usize, usize)>,
    delay: u64,
    objective: u64,
}

fn solve_core(
    graph: &[Vec<(usize, u64)>],
    allowed: &[bool],
    root: usize,
    sinks: &[usize],
    price: &[u64],
) -> Result<Option<Solution>, String> {
    let n = graph.len();
    if n == 0 || allowed.len() != n || price.len() != n || root >= n {
        return Err("bad graph dimensions".into());
    }
    if !allowed[root] || sinks.iter().any(|&s| s >= n || !allowed[s]) {
        return Ok(None);
    }
    let mut terminal_weights = BTreeMap::<usize, u64>::new();
    for &s in sinks {
        let w = terminal_weights.entry(s).or_insert(0);
        *w = w.checked_add(1).ok_or("duplicate sink weight overflow")?;
    }
    if terminal_weights.is_empty() {
        return Err("subset tree requires at least one sink".into());
    }
    let terminals: Vec<(usize, u64)> = terminal_weights.into_iter().collect();
    let k = terminals.len();
    if k > 6 {
        return Err("subset tree fanout exceeds configured kernel bound".into());
    }
    if n >= (MERGE_TAG as usize) {
        return Err("graph vertex ids exceed compact backpointer range".into());
    }
    let states = 1usize
        .checked_shl(k as u32)
        .ok_or("subset count overflow")?;
    let cells = states.checked_mul(n).ok_or("DP cell count overflow")?;
    let bytes = cells
        .checked_mul(std::mem::size_of::<u64>() * 2 + std::mem::size_of::<u32>())
        .ok_or("DP byte count overflow")?;
    if bytes > MEMORY_LIMIT_BYTES {
        return Err(format!(
            "DP needs {bytes} bytes, above {MEMORY_LIMIT_BYTES}-byte cap"
        ));
    }

    let mut weight_sum = vec![0u64; states];
    for mask in 1..states {
        let bit = mask.trailing_zeros() as usize;
        weight_sum[mask] = weight_sum[mask & (mask - 1)]
            .checked_add(terminals[bit].1)
            .ok_or("terminal weight sum overflow")?;
    }
    let mut dp = vec![INF; cells];
    let mut tie = vec![INF; cells];
    let mut action = vec![NONE; cells];
    for (bit, &(sink, _)) in terminals.iter().enumerate() {
        let ix = (1usize << bit) * n + sink;
        dp[ix] = price[sink];
        tie[ix] = 0;
        action[ix] = SEED;
    }

    for mask in 1usize..states {
        if mask.count_ones() > 1 {
            let mut left = (mask - 1) & mask;
            while left > 0 {
                let right = mask ^ left;
                if right > 0 && left < right {
                    let lo = left * n;
                    let ro = right * n;
                    let off = mask * n;
                    for v in 0..n {
                        if dp[lo + v] >= INF || dp[ro + v] >= INF {
                            continue;
                        }
                        let both = add(dp[lo + v], dp[ro + v]).ok_or("merge cost overflow")?;
                        let cand = both.checked_sub(price[v]).ok_or("merge price underflow")?;
                        let cand_tie = add(tie[lo + v], tie[ro + v]).ok_or("merge tie overflow")?;
                        if (cand, cand_tie) < (dp[off + v], tie[off + v]) {
                            dp[off + v] = cand;
                            tie[off + v] = cand_tie;
                            action[off + v] = MERGE_TAG | left as u32;
                        }
                    }
                }
                left = (left - 1) & mask;
            }
        }

        let off = mask * n;
        let move_tie = mask.count_ones() as u64;
        let mut heap = BinaryHeap::new();
        for v in 0..n {
            let ix = off + v;
            if dp[ix] < INF {
                heap.push(Reverse((dp[ix], tie[ix], v)));
            }
        }
        while let Some(Reverse((cost, path_tie, u))) = heap.pop() {
            let ix_u = off + u;
            if cost != dp[ix_u] || path_tie != tie[ix_u] {
                continue;
            }
            for &(v, delay) in &graph[u] {
                if v >= n || !allowed[v] {
                    continue;
                }
                let edge_cost = delay
                    .checked_mul(1024)
                    .and_then(|x| x.checked_mul(weight_sum[mask]))
                    .filter(|&x| x < INF)
                    .ok_or("weighted physical edge cost overflow")?;
                let cand = add(add(cost, edge_cost).ok_or("move cost overflow")?, price[v])
                    .ok_or("move price overflow")?;
                let cand_tie = add(path_tie, move_tie).ok_or("move tie overflow")?;
                let ix_v = off + v;
                if (cand, cand_tie) < (dp[ix_v], tie[ix_v]) {
                    dp[ix_v] = cand;
                    tie[ix_v] = cand_tie;
                    action[ix_v] = MOVE_TAG | u as u32;
                    heap.push(Reverse((cand, cand_tie, v)));
                }
            }
        }
    }

    let full = states - 1;
    let optimum = dp[full * n + root];
    if optimum >= INF {
        return Ok(None);
    }

    // Expand each DP state once. The secondary edge-count tie-break makes
    // shortest-path backpointers strictly progress even at zero-priced vertices.
    let mut raw_edges = BTreeSet::<(usize, usize)>::new();
    let mut seen_state = vec![false; cells];
    let mut stack = vec![(full, root)];
    while let Some((mask, v)) = stack.pop() {
        let ix = mask * n + v;
        if seen_state[ix] {
            continue;
        }
        seen_state[ix] = true;
        let a = action[ix];
        if a == SEED {
            if mask.count_ones() != 1 {
                return Err("non-singleton seed backpointer".into());
            }
            let bit = mask.trailing_zeros() as usize;
            if terminals[bit].0 != v {
                return Err("seed backpointer points at wrong terminal".into());
            }
        } else if a & MOVE_TAG != 0 {
            let u = (a & !MOVE_TAG) as usize;
            if u >= n || u == v {
                return Err("invalid move backpointer".into());
            }
            raw_edges.insert((u.min(v), u.max(v)));
            stack.push((mask, u));
        } else if a & MERGE_TAG != 0 {
            let left = (a & !MERGE_TAG) as usize;
            let right = mask ^ left;
            if left == 0 || right == 0 || (left & mask) != left {
                return Err("invalid merge backpointer".into());
            }
            stack.push((left, v));
            stack.push((right, v));
        } else {
            return Err(format!("missing backpointer at mask={mask} vertex={v}"));
        }
    }

    // Convert the backpointer union to a positive-delay SPT, then retain only
    // paths needed by the real sinks. Rebuild adjacency from the retained tree.
    let mut union_adj = vec![Vec::<(usize, u64)>::new(); n];
    for &(u, v) in &raw_edges {
        let delay = graph[u]
            .iter()
            .find_map(|&(to, w)| (to == v).then_some(w))
            .ok_or("backpointer edge absent from graph")?;
        union_adj[u].push((v, delay));
        union_adj[v].push((u, delay));
    }
    for row in &mut union_adj {
        row.sort_unstable();
    }
    let mut dist = vec![INF; n];
    let mut parent = vec![usize::MAX; n];
    let mut heap = BinaryHeap::new();
    dist[root] = 0;
    heap.push(Reverse((0u64, root)));
    while let Some(Reverse((d, u))) = heap.pop() {
        if d != dist[u] {
            continue;
        }
        for &(v, w) in &union_adj[u] {
            let nd = add(d, w).ok_or("SPT physical distance overflow")?;
            if nd < dist[v] {
                dist[v] = nd;
                parent[v] = u;
                heap.push(Reverse((nd, v)));
            }
        }
    }
    let mut retained_edges = BTreeSet::<(usize, usize)>::new();
    for &(sink, _) in &terminals {
        if dist[sink] >= INF {
            return Err(format!("reconstructed union disconnected sink {sink}"));
        }
        let mut u = sink;
        let mut steps = 0usize;
        while u != root {
            let p = parent[u];
            if p == usize::MAX || steps >= n {
                return Err("SPT path has missing/cyclic parent".into());
            }
            retained_edges.insert((p.min(u), p.max(u)));
            u = p;
            steps += 1;
        }
    }
    let mut vertices_set = BTreeSet::from([root]);
    for &(u, v) in &retained_edges {
        vertices_set.insert(u);
        vertices_set.insert(v);
    }
    let vertices: Vec<usize> = vertices_set.into_iter().collect();
    let edges: Vec<(usize, usize)> = retained_edges.into_iter().collect();
    if edges.len() + 1 != vertices.len() {
        return Err("retained SPT paths are not a tree".into());
    }
    if vertices.iter().any(|&v| !allowed[v]) {
        return Err("reconstruction included a foreign-pin vertex".into());
    }
    let delay = sinks.iter().try_fold(0u64, |sum, &s| {
        add(sum, dist[s]).ok_or("sink delay sum overflow")
    })?;
    let vertex_price = vertices.iter().try_fold(0u64, |sum, &v| {
        add(sum, price[v]).ok_or("tree vertex price overflow")
    })?;
    let actual = delay
        .checked_mul(1024)
        .and_then(|x| add(x, vertex_price))
        .ok_or("reconstructed objective overflow")?;
    if actual != optimum {
        return Err(format!(
            "DP/reconstruction mismatch: DP={optimum}, actual={actual}"
        ));
    }
    Ok(Some(Solution {
        vertices,
        edges,
        delay,
        objective: optimum,
    }))
}

pub(crate) fn route(
    inst: &Instance,
    net_index: usize,
    count: &[u32],
    history: &[u64],
    present: u64,
) -> Result<Option<(Route, u64)>, String> {
    let net = &inst.nets[net_index];
    let mut allowed = vec![true; inst.size];
    for (v, &owner) in inst.pins.iter().enumerate() {
        allowed[v] = owner < 0 || owner as usize == net_index;
    }
    let mut price = vec![0u64; inst.size];
    for v in 0..inst.size {
        let raw = history[v]
            .checked_add(
                present
                    .checked_mul(count[v] as u64)
                    .ok_or("present price overflow")?,
            )
            .ok_or("history price overflow")?;
        price[v] = inst.congestion_price(net_index, raw);
    }
    let solved = solve_core(&inst.neighbors, &allowed, net.driver, &net.sinks, &price)?;
    Ok(solved.map(|s| {
        (
            Route {
                vertices: s.vertices,
                edges: s.edges,
                delay: s.delay,
            },
            s.objective,
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn tiny_graph() -> Vec<Vec<(usize, u64)>> {
        // 2x2x2 layered grid, with nonuniform plane and via delays.
        let w = 2usize;
        let h = 2usize;
        let wh = w * h;
        let mut g = vec![Vec::new(); wh * 2];
        for z in 0..2 {
            for y in 0..h {
                for x in 0..w {
                    let u = (z * h + y) * w + x;
                    if x + 1 < w {
                        let v = u + 1;
                        let d = if z == 0 { 1 } else { 3 };
                        g[u].push((v, d));
                        g[v].push((u, d));
                    }
                    if y + 1 < h {
                        let v = u + w;
                        let d = if z == 0 { 2 } else { 1 };
                        g[u].push((v, d));
                        g[v].push((u, d));
                    }
                    if z == 0 {
                        let v = u + wh;
                        let d = 4;
                        g[u].push((v, d));
                        g[v].push((u, d));
                    }
                }
            }
        }
        for row in &mut g {
            row.sort_unstable();
        }
        g
    }

    fn exhaustive_cost(
        g: &[Vec<(usize, u64)>],
        allowed: &[bool],
        root: usize,
        sinks: &[usize],
        price: &[u64],
    ) -> u64 {
        let mut es = Vec::new();
        for u in 0..g.len() {
            for &(v, w) in &g[u] {
                if u < v {
                    es.push((u, v, w));
                }
            }
        }
        assert!(es.len() < usize::BITS as usize);
        let mut best = INF;
        for mask in 0usize..(1usize << es.len()) {
            let mut adj = vec![Vec::<(usize, u64)>::new(); g.len()];
            let mut edge_count = 0;
            for (i, &(u, v, w)) in es.iter().enumerate() {
                if (mask >> i) & 1 != 0 {
                    edge_count += 1;
                    adj[u].push((v, w));
                    adj[v].push((u, w));
                }
            }
            let mut d = vec![INF; g.len()];
            let mut parent = vec![usize::MAX; g.len()];
            let mut q = BinaryHeap::new();
            d[root] = 0;
            q.push(Reverse((0u64, root)));
            while let Some(Reverse((du, u))) = q.pop() {
                if du != d[u] {
                    continue;
                }
                for &(v, w) in &adj[u] {
                    if !allowed[v] {
                        continue;
                    }
                    let nd = du + w;
                    if nd < d[v] {
                        d[v] = nd;
                        parent[v] = u;
                        q.push(Reverse((nd, v)));
                    }
                }
            }
            if sinks.iter().any(|&s| d[s] >= INF) {
                continue;
            }
            let reached = d.iter().filter(|&&x| x < INF).count();
            if edge_count + 1 != reached {
                continue;
            }
            let used: Vec<usize> = (0..g.len()).filter(|&v| d[v] < INF).collect();
            let value = sinks.iter().map(|&s| d[s] * 1024).sum::<u64>()
                + used.iter().map(|&v| price[v]).sum::<u64>();
            best = best.min(value);
        }
        best
    }

    fn independent_shortest_delay(
        inst: &Instance,
        net_index: usize,
        source: usize,
        target: usize,
    ) -> u64 {
        let mut dist = vec![INF; inst.size];
        let mut heap = BinaryHeap::new();
        dist[source] = 0;
        heap.push(Reverse((0u64, source)));
        while let Some(Reverse((d, u))) = heap.pop() {
            if d != dist[u] {
                continue;
            }
            if u == target {
                return d;
            }
            for &(v, w) in &inst.neighbors[u] {
                if inst.pins[v] >= 0 && inst.pins[v] as usize != net_index {
                    continue;
                }
                let nd = d + w;
                if nd < dist[v] {
                    dist[v] = nd;
                    heap.push(Reverse((nd, v)));
                }
            }
        }
        INF
    }

    #[test]
    fn optional_designs_worst_four_sink_kernel_check() {
        let Ok(path) = std::env::var("M3D_SUBSET_TREE_PERF_INPUT") else {
            return;
        };
        let inst = Instance::load(&path);
        let (net_index, net) = inst
            .nets
            .iter()
            .enumerate()
            .filter(|(_, n)| n.sinks.len() == 4)
            .max_by_key(|(_, n)| {
                let mut xs = vec![n.driver % inst.w];
                let mut ys = vec![(n.driver / inst.w) % inst.h];
                let mut zs = vec![n.driver / inst.wh];
                for &s in &n.sinks {
                    xs.push(s % inst.w);
                    ys.push((s / inst.w) % inst.h);
                    zs.push(s / inst.wh);
                }
                (xs.iter().max().unwrap() - xs.iter().min().unwrap() + 1)
                    * (ys.iter().max().unwrap() - ys.iter().min().unwrap() + 1)
                    * (zs.iter().max().unwrap() - zs.iter().min().unwrap() + 1)
            })
            .expect("raw designs input must have a four-sink net");
        let count = vec![0u32; inst.size];
        let history = vec![0u64; inst.size];
        let started = Instant::now();
        let (route, objective) = route(&inst, net_index, &count, &history, 0)
            .unwrap()
            .expect("worst four-sink net must be routable");
        let expected = net
            .sinks
            .iter()
            .map(|&s| independent_shortest_delay(&inst, net_index, net.driver, s))
            .sum::<u64>()
            * 1024;
        assert_eq!(
            objective, expected,
            "zero-price four-sink optimum must equal sum of independent shortest paths"
        );
        assert_eq!(route.delay * 1024, expected);
        assert!(route
            .vertices
            .iter()
            .all(|&v| inst.pins[v] < 0 || inst.pins[v] as usize == net_index));
        eprintln!("subset-tree perf raw_net={} fanout=4 vertices={} edges={} objective={} expected={} elapsed_ms={}",net.id,route.vertices.len(),route.edges.len(),objective,expected,started.elapsed().as_millis());
    }
    #[test]
    fn optional_hard_case_07_worst_six_sink_kernel_check() {
        let Ok(path) = std::env::var("M3D_SUBSET_TREE6_PERF_INPUT") else {
            return;
        };
        let inst = Instance::load(&path);
        let (net_index, net) = inst
            .nets
            .iter()
            .enumerate()
            .filter(|(_, n)| n.sinks.len() == 6)
            .max_by_key(|(_, n)| {
                let mut xs = vec![n.driver % inst.w];
                let mut ys = vec![(n.driver / inst.w) % inst.h];
                let mut zs = vec![n.driver / inst.wh];
                for &s in &n.sinks {
                    xs.push(s % inst.w);
                    ys.push((s / inst.w) % inst.h);
                    zs.push(s / inst.wh);
                }
                (xs.iter().max().unwrap() - xs.iter().min().unwrap() + 1)
                    * (ys.iter().max().unwrap() - ys.iter().min().unwrap() + 1)
                    * (zs.iter().max().unwrap() - zs.iter().min().unwrap() + 1)
            })
            .expect("hard case_07 raw input must have a six-sink net");
        let count = vec![0u32; inst.size];
        let history = vec![0u64; inst.size];
        let started = Instant::now();
        let (route, objective) = route(&inst, net_index, &count, &history, 0)
            .unwrap()
            .expect("worst six-sink net must be routable");
        let expected = net
            .sinks
            .iter()
            .map(|&s| independent_shortest_delay(&inst, net_index, net.driver, s))
            .sum::<u64>()
            * 1024;
        assert_eq!(
            objective, expected,
            "zero-price six-sink optimum must equal sum of independent shortest paths"
        );
        assert_eq!(route.delay * 1024, expected);
        assert!(route
            .vertices
            .iter()
            .all(|&v| inst.pins[v] < 0 || inst.pins[v] as usize == net_index));
        eprintln!("subset-tree perf raw_net={} fanout=6 vertices={} edges={} objective={} expected={} elapsed_ms={}",net.id,route.vertices.len(),route.edges.len(),objective,expected,started.elapsed().as_millis());
    }

    #[test]
    fn matches_exhaustive_five_and_six_terminal_trees_with_physical_scale_prices() {
        let g = tiny_graph();
        let allowed = vec![true; g.len()];
        let mut price = vec![0u64; g.len()];
        price[1] = 4096;
        price[2] = 2048;
        price[4] = 4096;
        price[6] = 1024;
        for sinks in [vec![1, 2, 3, 4, 5], vec![1, 2, 3, 4, 5, 7]] {
            let got = solve_core(&g, &allowed, 0, &sinks, &price)
                .unwrap()
                .unwrap();
            assert_eq!(
                got.objective,
                exhaustive_cost(&g, &allowed, 0, &sinks, &price),
                "sink count {}",
                sinks.len()
            );
            assert_eq!(got.edges.len() + 1, got.vertices.len());
            assert_eq!(
                got.objective,
                got.delay * 1024 + got.vertices.iter().map(|&v| price[v]).sum::<u64>()
            );
        }
    }

    #[test]
    fn matches_exhaustive_two_and_three_terminal_trees() {
        let g = tiny_graph();
        let allowed = vec![true; g.len()];
        let price: Vec<u64> = (0..g.len()).map(|v| ((v * 7) % 5) as u64).collect();
        for (root, sinks) in [(0, vec![3, 5]), (0, vec![3, 5, 7]), (6, vec![1, 2, 4])] {
            let got = solve_core(&g, &allowed, root, &sinks, &price)
                .unwrap()
                .unwrap();
            assert_eq!(
                got.objective,
                exhaustive_cost(&g, &allowed, root, &sinks, &price)
            );
            assert_eq!(got.edges.len() + 1, got.vertices.len());
        }
    }

    #[test]
    fn foreign_pin_is_hard_and_root_price_is_once() {
        let g = tiny_graph();
        let mut allowed = vec![true; g.len()];
        allowed[1] = false;
        let mut price = vec![0; g.len()];
        price[0] = 13;
        price[2] = 9;
        let got = solve_core(&g, &allowed, 0, &[3, 5], &price)
            .unwrap()
            .unwrap();
        assert!(!got.vertices.contains(&1));
        let exact = got.delay * 1024 + got.vertices.iter().map(|&v| price[v]).sum::<u64>();
        assert_eq!(got.objective, exact);
        assert!(got.vertices.contains(&0));
        // A concrete feasible incumbent tree cannot beat the exact priced DP.
        let incumbent_edges = [(0usize, 2usize), (2, 3), (0, 4), (4, 5)];
        let incumbent_vertices = [0usize, 2, 3, 4, 5];
        let incumbent_delay = 3 + 7; // d(0,3)=3 and d(0,5)=7, in physical units
        let incumbent_objective =
            incumbent_delay * 1024 + incumbent_vertices.iter().map(|&v| price[v]).sum::<u64>();
        assert_eq!(incumbent_edges.len() + 1, incumbent_vertices.len());
        assert!(incumbent_objective >= got.objective);
    }

    #[test]
    fn zero_prices_and_duplicate_sink_preserve_objective() {
        let g = tiny_graph();
        let allowed = vec![true; g.len()];
        let price = vec![0; g.len()];
        let got = solve_core(&g, &allowed, 0, &[3, 3, 5], &price)
            .unwrap()
            .unwrap();
        assert_eq!(got.objective, got.delay * 1024);
        assert_eq!(
            got.objective,
            exhaustive_cost(&g, &allowed, 0, &[3, 3, 5], &price)
        );
    }
}
