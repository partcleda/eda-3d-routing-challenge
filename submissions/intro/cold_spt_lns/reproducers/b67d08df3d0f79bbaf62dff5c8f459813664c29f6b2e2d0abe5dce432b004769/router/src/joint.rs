//! Joint selection of freshly generated, mutually compatible route candidates.
//! All candidates come from this invocation's incumbent and shortest-path calls.
//! The selection is exact over this finite pool, not over all possible trees.
use super::*;

#[derive(Clone)]
struct Candidate {
    verts: Vec<u32>,
    edges: Vec<(u32, u32)>,
    delay: u64,
}

fn insert(pool: &mut Vec<Candidate>, mut candidate: Candidate) {
    candidate.edges.sort_unstable();
    if !pool.iter().any(|old| old.edges == candidate.edges) {
        pool.push(candidate);
    }
}

fn select(
    pools: &[Vec<Candidate>],
    owner: &mut [i32],
    old_total: u64,
    old_vertices: usize,
    neutral: u64,
    rng: &mut Rng,
) -> Option<Vec<usize>> {
    let mut suffix = vec![0; pools.len() + 1];
    for i in (0..pools.len()).rev() {
        suffix[i] = suffix[i + 1] + pools[i].iter().map(|c| c.delay).min().unwrap();
    }
    let mut best = (old_total, old_vertices, None, 0u64);
    let mut choices = vec![0; pools.len()];
    fn visit(
        pools: &[Vec<Candidate>],
        owner: &mut [i32],
        suffix: &[u64],
        depth: usize,
        delay: u64,
        vertices: usize,
        choices: &mut [usize],
        best: &mut (u64, usize, Option<Vec<usize>>, u64),
        neutral: u64,
        rng: &mut Rng,
    ) {
        if delay + suffix[depth] > best.0 {
            return;
        }
        if depth == pools.len() {
            if delay < best.0 || (delay == best.0 && neutral != 2 && vertices < best.1) {
                *best = (delay, vertices, Some(choices.to_vec()), 1);
            } else if neutral > 0 && delay == best.0 && (neutral == 2 || vertices == best.1) {
                best.3 += 1;
                if best.3 == 1 || rng.below(best.3) == 0 {
                    best.1 = vertices;
                    best.2 = Some(choices.to_vec());
                }
            }
            return;
        }
        for (index, candidate) in pools[depth].iter().enumerate() {
            if delay + candidate.delay + suffix[depth + 1] > best.0 {
                break;
            }
            if candidate.verts.iter().any(|&v| owner[v as usize] >= 0) {
                continue;
            }
            for &v in &candidate.verts {
                owner[v as usize] = 0;
            }
            choices[depth] = index;
            visit(
                pools,
                owner,
                suffix,
                depth + 1,
                delay + candidate.delay,
                vertices + candidate.verts.len(),
                choices,
                best,
                neutral,
                rng,
            );
            for &v in &candidate.verts {
                owner[v as usize] = -1;
            }
        }
    }
    visit(
        pools,
        owner,
        &suffix,
        0,
        0,
        0,
        &mut choices,
        &mut best,
        neutral,
        rng,
    );
    best.2
}

pub(super) fn group_move(
    inst: &Inst,
    sol: &mut Solution,
    owner: &mut [i32],
    ws: &mut Scratch,
    group: &[usize],
    compact_shortest_tree: bool,
    neutral: u64,
) -> Option<(i64, bool)> {
    if group.len() < 2 || group.len() > 6 {
        return None;
    }
    let saved: Vec<_> = group
        .iter()
        .map(|&nid| Candidate {
            verts: sol.verts[nid].clone(),
            edges: sol.edges[nid].clone(),
            delay: sol.delay[nid],
        })
        .collect();
    for route in &saved {
        uncommit(owner, &route.verts);
    }
    let mut pools = vec![Vec::new(); group.len()];
    let mut ideals = Vec::new();
    for (i, &nid) in group.iter().enumerate() {
        insert(&mut pools[i], saved[i].clone());
        ws.dijkstra(
            inst,
            owner,
            inst.nets[nid].driver,
            nid as i32,
            false,
            u32::MAX,
        );
        let ideal = ws
            .extract(inst, &inst.nets[nid])
            .map(|(verts, edges, delay)| Candidate {
                verts,
                edges,
                delay,
            });
        if let Some(ref route) = ideal {
            insert(&mut pools[i], route.clone());
        }
        // A second exact extraction explores equal-delay tree sharing choices.
        if let Some((verts, edges, delay)) = ws.extract(inst, &inst.nets[nid]) {
            insert(
                &mut pools[i],
                Candidate {
                    verts,
                    edges,
                    delay,
                },
            );
        }
        if compact_shortest_tree {
            for farthest_first in [false, true] {
                let salt = ws.rng.next();
                if let Some((verts, edges, delay)) =
                    ws.extract_compact(inst, &inst.nets[nid], salt, farthest_first)
                {
                    insert(
                        &mut pools[i],
                        Candidate {
                            verts,
                            edges,
                            delay,
                        },
                    );
                }
            }
        }
        ideals.push(ideal);
    }
    let mut soft = vec![false; inst.nets.len()];
    for &nid in group {
        soft[nid] = true;
    }
    for (i, &nid) in group.iter().enumerate() {
        let mut marked = Vec::new();
        for (j, ideal) in ideals.iter().enumerate() {
            if i == j {
                continue;
            }
            if let Some(route) = ideal {
                for &v in &route.verts {
                    if owner[v as usize] < 0 {
                        owner[v as usize] = group[j] as i32;
                        marked.push(v);
                    }
                }
            }
        }
        // Yield completely, or pay a soft price for using the other ideals.
        ws.dijkstra(
            inst,
            owner,
            inst.nets[nid].driver,
            nid as i32,
            false,
            u32::MAX,
        );
        if let Some((verts, edges, delay)) = ws.extract(inst, &inst.nets[nid]) {
            insert(
                &mut pools[i],
                Candidate {
                    verts,
                    edges,
                    delay,
                },
            );
        }
        for price in [0.5, 2.0] {
            ws.direct_chain_prices = Some(false);
            ws.entry_owner.clear();
            ws.pdijkstra(
                inst,
                owner,
                &soft,
                nid as i32,
                inst.nets[nid].driver,
                price,
                1.0,
            );
            if let Some((verts, edges)) =
                ws.extract_pen_with_owner(inst, &inst.nets[nid], price, 1.0, Some(owner))
            {
                let delay = physical_tree_delay(inst, nid, &edges);
                insert(
                    &mut pools[i],
                    Candidate {
                        verts,
                        edges,
                        delay,
                    },
                );
            }
            ws.direct_chain_prices = None;
        }
        for v in marked {
            owner[v as usize] = -1;
        }
    }
    for pool in &mut pools {
        pool.sort_by_key(|c| (c.delay, c.verts.len()));
    }
    let old_total = saved.iter().map(|c| c.delay).sum();
    let old_vertices: usize = saved.iter().map(|c| c.verts.len()).sum();
    let choices = select(&pools, owner, old_total, old_vertices, neutral, &mut ws.rng);
    let mut total = 0;
    let mut vertices = 0;
    for (i, &nid) in group.iter().enumerate() {
        let route = choices
            .as_ref()
            .map_or(&saved[i], |pick| &pools[i][pick[i]]);
        commit(owner, nid, &route.verts);
        sol.verts[nid] = route.verts.clone();
        sol.edges[nid] = route.edges.clone();
        sol.delay[nid] = route.delay;
        total += route.delay;
        vertices += route.verts.len();
    }
    assert!(total <= old_total);
    sol.total = sol.total - old_total + total;
    sol.vcount = sol.vcount - old_vertices as u64 + vertices as u64;
    Some((total as i64 - old_total as i64, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_matches_exhaustive_compatible_combinations() {
        let mut rng = Rng::new(491);
        for trial in 0..200 {
            let count = 2 + trial % 5;
            let mut pools = Vec::new();
            for _ in 0..count {
                let mut pool = Vec::new();
                for _ in 0..4 {
                    let mut verts = vec![rng.below(12) as u32, rng.below(12) as u32];
                    verts.sort_unstable();
                    verts.dedup();
                    pool.push(Candidate {
                        verts,
                        edges: Vec::new(),
                        delay: 1 + rng.below(30),
                    });
                }
                pool.sort_by_key(|c| (c.delay, c.verts.len()));
                pools.push(pool);
            }
            let mut exact = (u64::MAX, usize::MAX);
            for mask in 0..4usize.pow(count as u32) {
                let indices: Vec<_> = (0..count)
                    .map(|i| mask / 4usize.pow(i as u32) % 4)
                    .collect();
                let mut all = vec![11]; // one resource belongs to a fixed outside net
                let mut cost = 0;
                let mut size = 0;
                for i in 0..count {
                    let c = &pools[i][indices[i]];
                    all.extend(c.verts.iter().copied());
                    cost += c.delay;
                    size += c.verts.len();
                }
                let before = all.len();
                all.sort_unstable();
                all.dedup();
                if all.len() == before {
                    exact = exact.min((cost, size));
                }
            }
            let mut owner = vec![-1; 12];
            owner[11] = 7;
            let picked = select(&pools, &mut owner, u64::MAX, usize::MAX, 0, &mut rng);
            assert_eq!(owner, [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 7]);
            let got = picked.map(|p| {
                (
                    p.iter().enumerate().map(|(i, &j)| pools[i][j].delay).sum(),
                    p.iter()
                        .enumerate()
                        .map(|(i, &j)| pools[i][j].verts.len())
                        .sum(),
                )
            });
            assert_eq!(
                got,
                if exact.0 == u64::MAX {
                    None
                } else {
                    Some(exact)
                }
            );
        }
    }

    #[test]
    fn neutral_selection_explores_ties_without_increasing_delay() {
        let pools = vec![
            vec![
                Candidate {
                    verts: vec![0],
                    edges: vec![],
                    delay: 1,
                },
                Candidate {
                    verts: vec![1, 4],
                    edges: vec![],
                    delay: 1,
                },
            ],
            vec![
                Candidate {
                    verts: vec![2],
                    edges: vec![],
                    delay: 1,
                },
                Candidate {
                    verts: vec![3],
                    edges: vec![],
                    delay: 1,
                },
            ],
        ];
        for mode in 0..=2 {
            let mut observed = std::collections::BTreeSet::new();
            for seed in 0..128 {
                let mut owner = vec![-1; 6];
                owner[5] = 77;
                let pick = select(&pools, &mut owner, 2, 2, mode, &mut Rng::new(seed));
                assert_eq!(owner, [-1, -1, -1, -1, -1, 77]);
                if mode == 0 {
                    assert!(pick.is_none());
                    continue;
                }
                let pick = pick.unwrap();
                assert_eq!(pools[0][pick[0]].delay + pools[1][pick[1]].delay, 2);
                if mode == 1 {
                    assert_eq!(pick[0], 0);
                }
                observed.insert(pick);
            }
            assert_eq!(
                observed.len(),
                match mode {
                    0 => 0,
                    1 => 2,
                    _ => 4,
                }
            );
        }
    }
}
