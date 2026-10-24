//! Experimental candidates generated only from this invocation's own incumbent.
//! The exported pool is research data, not a globally legal submission.
use super::*;

pub fn write_pool(instance: &str, incumbent: &str, path: &str, seed: u64) {
    let mut inst = load_inst(instance);
    inst.early_stop = true;
    inst.use_radix = true;
    inst.use_dial = true;
    inst.use_astar = true;
    let sol = load_solution(&inst, incumbent).expect("own generated incumbent must be legal");
    let mut owner = vec![-1; inst.n as usize];
    for nid in 0..inst.nets.len() {
        commit(&mut owner, nid, &sol.verts[nid]);
    }
    let soft = vec![true; inst.nets.len()];
    let mut ws = Scratch::new(inst.n, inst.nets.len(), seed);
    let mut nets = Vec::new();
    for nid in 0..inst.nets.len() {
        uncommit(&mut owner, &sol.verts[nid]);
        let mut candidates = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        let mut add = |mut edges: Vec<(u32, u32)>| {
            edges.sort_unstable();
            if seen.insert(edges.clone()) {
                let delay = physical_tree_delay(&inst, nid, &edges);
                let geometry: Vec<_> = edges
                    .iter()
                    .map(|&(u, v)| (inst.xyz(u), inst.xyz(v)))
                    .collect();
                candidates.push(serde_json::json!({"edges":geometry,"delay":delay}));
            }
        };
        add(sol.edges[nid].clone());
        ws.dijkstra(
            &inst,
            &owner,
            inst.nets[nid].driver,
            nid as i32,
            true,
            u32::MAX,
        );
        for _ in 0..2 {
            if let Some((_, edges, _)) = ws.extract(&inst, &inst.nets[nid]) {
                add(edges);
            }
        }
        for farthest in [false, true] {
            let salt = ws.rng.next();
            if let Some((_, edges, _)) = ws.extract_compact(&inst, &inst.nets[nid], salt, farthest)
            {
                add(edges);
            }
        }
        // Different prices expose coordinated displacement alternatives. Foreign
        // pins remain hard obstacles even when all routed occupancy is soft.
        for price in [0.25, 1.0, 4.0, 16.0, 64.0] {
            ws.direct_chain_prices = Some(false);
            ws.entry_owner.clear();
            ws.pdijkstra(
                &inst,
                &owner,
                &soft,
                nid as i32,
                inst.nets[nid].driver,
                price,
                1.0,
            );
            for _ in 0..2 {
                if let Some((_, edges)) =
                    ws.extract_pen_with_owner(&inst, &inst.nets[nid], price, 1.0, Some(&owner))
                {
                    add(edges);
                }
            }
            ws.direct_chain_prices = None;
        }
        commit(&mut owner, nid, &sol.verts[nid]);
        nets.push(serde_json::json!({"net":nid,"candidates":candidates}));
    }
    let value = serde_json::json!({"format":"cold-spt-research-pool-v1","instance":inst.name,
        "incumbent_delay":sol.total,"nets":nets});
    serde_json::to_writer(BufWriter::new(File::create(path).unwrap()), &value).unwrap();
    eprintln!(
        "POOL_FINAL nets={} incumbent={}",
        inst.nets.len(),
        sol.total
    );
}
