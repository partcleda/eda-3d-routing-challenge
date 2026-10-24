//! Greedy footprint reduction inside the exact shortest-path predecessor DAG.
//! Each added branch preserves every sink's physical shortest-path distance.
//! This is a candidate heuristic, not an optimal Steiner arborescence solver.
use super::*;
use std::collections::{HashMap, HashSet};

impl Scratch {
    pub(super) fn extract_compact(
        &self,
        inst: &Inst,
        net: &Net,
        salt: u64,
        farthest_first: bool,
    ) -> Option<(Vec<u32>, Vec<(u32, u32)>, u64)> {
        if net
            .sinks
            .iter()
            .any(|&s| self.stamp[s as usize] != self.cur)
        {
            return None;
        }
        let mut seen = HashSet::new();
        let mut stack = net.sinks.clone();
        stack.push(net.driver);
        let mut nb = [0; 6];
        let mut wb = [0; 6];
        while let Some(v) = stack.pop() {
            if !seen.insert(v) || v == net.driver {
                continue;
            }
            let k = inst.neigh(v, &mut nb, &mut wb);
            for j in 0..k {
                let u = nb[j] as usize;
                if self.stamp[u] == self.cur
                    && self.dist[u] < INF
                    && self.dist[u].checked_add(wb[j]) == Some(self.dist[v as usize])
                {
                    stack.push(u as u32);
                }
            }
        }
        let mut vertices: Vec<_> = seen.into_iter().collect();
        vertices.sort_unstable_by_key(|&v| (self.dist[v as usize], v));
        let index: HashMap<_, _> = vertices.iter().enumerate().map(|(i, &v)| (v, i)).collect();
        let root = *index.get(&net.driver)?;
        let mut predecessors = vec![Vec::new(); vertices.len()];
        for (i, &v) in vertices.iter().enumerate() {
            let k = inst.neigh(v, &mut nb, &mut wb);
            for j in 0..k {
                if let Some(&p) = index.get(&nb[j]) {
                    if self.dist[nb[j] as usize].checked_add(wb[j]) == Some(self.dist[v as usize]) {
                        assert!(p < i, "physical routing weights must be positive");
                        predecessors[i].push(p);
                    }
                }
            }
        }
        let mut sinks = net.sinks.clone();
        sinks.sort_unstable_by_key(|&v| (self.dist[v as usize], v));
        if farthest_first {
            sinks.reverse();
        }
        let mut in_tree = vec![false; vertices.len()];
        in_tree[root] = true;
        let mut edges = Vec::new();
        let mut cost = vec![usize::MAX; vertices.len()];
        let mut parent = vec![usize::MAX; vertices.len()];
        for sink in sinks {
            let sink_index = index[&sink];
            if in_tree[sink_index] {
                continue;
            }
            for i in 0..vertices.len() {
                if in_tree[i] {
                    cost[i] = 0;
                    continue;
                }
                cost[i] = usize::MAX;
                parent[i] = usize::MAX;
                for &p in &predecessors[i] {
                    if cost[p] == usize::MAX {
                        continue;
                    }
                    // Stateless tie-breaking keeps this helper independent of
                    // hash iteration order and makes identical inputs replayable.
                    let rank = |p: usize| {
                        let mut x = salt ^ ((vertices[p] as u64) << 32) ^ vertices[i] as u64;
                        x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
                        x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
                        x ^ (x >> 31)
                    };
                    let candidate = cost[p] + 1;
                    if candidate < cost[i]
                        || (candidate == cost[i]
                            && (parent[i] == usize::MAX
                                || (rank(p), p) < (rank(parent[i]), parent[i])))
                    {
                        cost[i] = candidate;
                        parent[i] = p;
                    }
                }
            }
            let mut i = sink_index;
            while !in_tree[i] {
                let p = parent[i];
                if p == usize::MAX {
                    return None;
                }
                edges.push(canon(vertices[i], vertices[p]));
                in_tree[i] = true;
                i = p;
            }
        }
        let delay = net
            .sinks
            .iter()
            .map(|&s| self.dist[s as usize] as u64)
            .sum();
        let mut used: Vec<_> = vertices
            .iter()
            .enumerate()
            .filter_map(|(i, &v)| in_tree[i].then_some(v))
            .collect();
        used.sort_unstable();
        edges.sort_unstable();
        assert_eq!(used.len(), edges.len() + 1);
        Some((used, edges, delay))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_tree_keeps_exact_delays_and_avoids_foreign_resources() {
        let mut inst = Inst {
            early_stop: false,
            retain_negotiated: false,
            use_radix: true,
            use_dial: false,
            use_astar: false,
            budget_bound: false,
            free_dist: Vec::new(),
            check_state: false,
            name: "compact-tiny".into(),
            w: 3,
            h: 3,
            l: 1,
            wh: 9,
            n: 9,
            ld: vec![2],
            vd: 3,
            nets: vec![Net {
                driver: 0,
                sinks: vec![6, 8],
            }],
            pin_owner: vec![-1; 9],
        };
        for &p in &[0, 6, 8] {
            inst.pin_owner[p] = 0;
        }
        for block in [None, Some(1), Some(3), Some(4), Some(7)] {
            let mut owner = vec![-1; 9];
            if let Some(b) = block {
                owner[b] = 1;
                inst.pin_owner[b] = 1;
            }
            let mut ws = Scratch::new(9, 1, 93);
            ws.dijkstra(&inst, &owner, 0, 0, false, u32::MAX);
            let exact = ws.extract(&inst, &inst.nets[0]).unwrap().2;
            for salt in 0..16 {
                for farthest in [false, true] {
                    let (verts, edges, delay) = ws
                        .extract_compact(&inst, &inst.nets[0], salt, farthest)
                        .unwrap();
                    assert_eq!(delay, exact);
                    assert_eq!(edges.len() + 1, verts.len());
                    assert!(block.map_or(true, |b| !verts.contains(&(b as u32))));
                    assert_eq!(physical_tree_delay(&inst, 0, &edges), exact);
                    let mut adj = vec![Vec::new(); 9];
                    for &(u, v) in &edges {
                        adj[u as usize].push(v);
                        adj[v as usize].push(u);
                    }
                    let mut distance = vec![u32::MAX; 9];
                    distance[0] = 0;
                    let mut pending = vec![0];
                    while let Some(u) = pending.pop() {
                        for &v in &adj[u] {
                            if distance[v as usize] == u32::MAX {
                                distance[v as usize] =
                                    distance[u] + edge_w(&inst, u as u32, v).unwrap();
                                pending.push(v as usize);
                            }
                        }
                    }
                    for &s in &inst.nets[0].sinks {
                        assert_eq!(distance[s as usize], ws.dist[s as usize]);
                    }
                    if block.is_none() && !farthest {
                        assert_eq!(verts.len(), 5);
                    }
                }
            }
            if let Some(b) = block {
                inst.pin_owner[b] = -1;
            }
        }
    }
}
