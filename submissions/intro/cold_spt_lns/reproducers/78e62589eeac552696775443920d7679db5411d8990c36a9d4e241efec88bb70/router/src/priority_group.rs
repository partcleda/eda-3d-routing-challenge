//! Private static, multi-sink adaptation of priority-based search.
//! Branch on conflicting nets' priorities, then recompute affected descendants.
//! Fixed node budget; incomplete search; node costs are NOT descendant bounds.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static CALLS: AtomicU64 = AtomicU64::new(0);
static EXPANDED: AtomicU64 = AtomicU64::new(0);
static LOW_LEVEL: AtomicU64 = AtomicU64::new(0);
static ACCEPTED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Route {
    verts: Vec<u32>,
    edges: Vec<(u32, u32)>,
    delay: u64,
}

#[derive(Clone)]
struct Node {
    // Bit j in reach[i] means i has higher priority than j, transitively.
    reach: Vec<u64>,
    routes: Vec<Arc<Route>>,
    cost: u64,
}

#[derive(Default, Debug)]
struct Stats {
    expanded: usize,
    low_level: usize,
    decreasing_children: usize,
}

fn add_priority(reach: &[u64], high: usize, low: usize) -> Option<Vec<u64>> {
    if high == low || reach[low] & (1u64 << high) != 0 {
        return None;
    }
    let mut result = reach.to_vec();
    let descendants = reach[low] | (1u64 << low);
    for i in 0..reach.len() {
        if i == high || reach[i] & (1u64 << high) != 0 {
            result[i] |= descendants;
        }
    }
    Some(result)
}

fn overlap(a: &[u32], b: &[u32]) -> usize {
    let (mut i, mut j, mut count) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => { i += 1; j += 1; count += 1; }
        }
    }
    count
}

fn conflict(routes: &[Arc<Route>]) -> Option<(usize, usize)> {
    let (mut best, mut most) = (None, 0);
    for i in 0..routes.len() {
        for j in i + 1..routes.len() {
            let count = overlap(&routes[i].verts, &routes[j].verts);
            if count > most { best = Some((i, j)); most = count; }
        }
    }
    best
}

fn low_level(
    inst: &Inst, owner: &mut [i32], ws: &mut Scratch, nid: usize,
    higher: impl Iterator<Item = Arc<Route>>, reverse: bool, stats: &mut Stats,
) -> Option<Arc<Route>> {
    let mut marked = Vec::new();
    for route in higher {
        for &v in &route.verts {
            if owner[v as usize] < 0 {
                owner[v as usize] = i32::MAX;
                marked.push(v);
            }
        }
    }
    stats.low_level += 1;
    ws.dijkstra(inst, owner, inst.nets[nid].driver, nid as i32, false, u32::MAX);
    let route = ws.extract_stable_order(inst, &inst.nets[nid], reverse);
    for v in marked { owner[v as usize] = -1; }
    route.map(|(verts, edges, delay)| Arc::new(Route { verts, edges, delay }))
}

fn update(
    inst: &Inst, owner: &mut [i32], ws: &mut Scratch, group: &[usize],
    node: &mut Node, lower: usize, reverse: bool, stats: &mut Stats,
) -> bool {
    let count = group.len();
    let affected = node.reach[lower] | (1u64 << lower);
    let mut done = 0u64;
    // A deterministic topological traversal; incomparable nets do not constrain
    // one another. Recompute every affected descendant, even if still feasible,
    // so freed ancestor geometry can also make its route cheaper.
    for _ in 0..count {
        let next = (0..count).find(|&j| {
            done & (1u64 << j) == 0 && (0..count).all(|i|
                node.reach[i] & (1u64 << j) == 0 || done & (1u64 << i) != 0)
        }).expect("priority graph must be acyclic");
        done |= 1u64 << next;
        if affected & (1u64 << next) == 0 { continue; }
        let ancestors: Vec<_> = (0..count).filter(|&i| node.reach[i] & (1u64 << next) != 0)
            .map(|i| Arc::clone(&node.routes[i])).collect();
        let Some(route) = low_level(inst, owner, ws, group[next], ancestors.into_iter(), reverse, stats) else {
            return false;
        };
        node.routes[next] = route;
    }
    node.cost = node.routes.iter().map(|r| r.delay).sum();
    true
}

fn search(
    inst: &Inst, owner: &mut [i32], ws: &mut Scratch, group: &[usize],
    cap: u64, old_vertices: usize, neutral: u64, reverse: bool, max_nodes: usize,
) -> (Option<Vec<Arc<Route>>>, Stats) {
    let mut stats = Stats::default();
    if max_nodes == 0 || group.is_empty() || group.len() > 16 { return (None, stats); }
    let mut routes = Vec::new();
    for &nid in group {
        let Some(route) = low_level(inst, owner, ws, nid, std::iter::empty(), reverse, &mut stats) else {
            return (None, stats);
        };
        routes.push(route);
    }
    let cost = routes.iter().map(|r| r.delay).sum();
    // Only this independent-net root cost is an admissible lower bound.
    if cost > cap { return (None, stats); }
    let root = Node { reach: vec![0; group.len()], routes, cost };
    let mut seen = std::collections::HashSet::new();
    seen.insert(root.reach.clone());
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if stats.expanded >= max_nodes { break; }
        stats.expanded += 1;
        let Some((a, b)) = conflict(&node.routes) else {
            let vertices: usize = node.routes.iter().map(|r| r.verts.len()).sum();
            let useful = node.cost < cap || (node.cost == cap &&
                (neutral == 2 || vertices < old_vertices || (neutral == 1 && vertices == old_vertices)));
            if useful { return (Some(node.routes), stats); }
            continue;
        };
        assert_eq!(node.reach[a] & (1u64 << b), 0, "ordered routes must not collide");
        assert_eq!(node.reach[b] & (1u64 << a), 0, "ordered routes must not collide");
        let mut children = Vec::new();
        for (high, low) in [(a, b), (b, a)] {
            let Some(reach) = add_priority(&node.reach, high, low) else { continue; };
            if !seen.insert(reach.clone()) { continue; }
            let mut child = Node { reach, routes: node.routes.clone(), cost: 0 };
            if update(inst, owner, ws, group, &mut child, low, reverse, &mut stats) {
                if child.cost < node.cost { stats.decreasing_children += 1; }
                // Never prune on child.cost: changing an ancestor later can
                // reduce descendant costs. It orders DFS siblings only.
                children.push(child);
            }
        }
        children.sort_by_key(|n| std::cmp::Reverse(n.cost));
        stack.extend(children);
    }
    (None, stats)
}

pub(super) fn group_move(
    inst: &Inst, sol: &mut Solution, owner: &mut [i32], ws: &mut Scratch,
    group: &[usize], neutral: u64, max_nodes: usize,
) -> Option<(i64, bool)> {
    if group.len() < 2 || group.len() > 16 { return None; }
    CALLS.fetch_add(1, Ordering::Relaxed);
    let saved: Vec<_> = group.iter().map(|&nid| Route {
        verts: sol.verts[nid].clone(), edges: sol.edges[nid].clone(), delay: sol.delay[nid],
    }).collect();
    for route in &saved { uncommit(owner, &route.verts); }
    let old_cost: u64 = saved.iter().map(|r| r.delay).sum();
    let old_vertices: usize = saved.iter().map(|r| r.verts.len()).sum();
    let fixed_before = if inst.check_state { Some(owner.to_vec()) } else { None };
    let reverse = ws.rng.below(2) != 0;
    let (selected, stats) = search(inst, owner, ws, group, old_cost, old_vertices, neutral, reverse, max_nodes);
    if let Some(before) = fixed_before { assert_eq!(owner, before.as_slice()); }
    EXPANDED.fetch_add(stats.expanded as u64, Ordering::Relaxed);
    LOW_LEVEL.fetch_add(stats.low_level as u64, Ordering::Relaxed);
    let mut new_cost = 0;
    let mut new_vertices = 0;
    for (i, &nid) in group.iter().enumerate() {
        let route = selected.as_ref().map_or(&saved[i], |r| r[i].as_ref());
        commit(owner, nid, &route.verts);
        sol.verts[nid] = route.verts.clone();
        sol.edges[nid] = route.edges.clone();
        sol.delay[nid] = route.delay;
        new_cost += route.delay;
        new_vertices += route.verts.len();
    }
    assert!(new_cost <= old_cost);
    sol.total = sol.total - old_cost + new_cost;
    sol.vcount = sol.vcount - old_vertices as u64 + new_vertices as u64;
    if selected.is_some() {
        ACCEPTED.fetch_add(1, Ordering::Relaxed);
        Some((new_cost as i64 - old_cost as i64, true))
    } else { None }
}

pub(super) fn print_stats() {
    eprintln!("PRIORITY_STATS cumulative_calls={} expanded={} low_level={} accepted={}",
        CALLS.load(Ordering::Relaxed), EXPANDED.load(Ordering::Relaxed),
        LOW_LEVEL.load(Ordering::Relaxed), ACCEPTED.load(Ordering::Relaxed));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(nets: Vec<Net>, pins: Vec<i32>) -> Inst {
        let mut inst = Inst {
            early_stop:true,retain_negotiated:false,use_radix:true,use_dial:true,use_astar:true,
            budget_bound:false,free_dist:Vec::new(),check_state:true,name:"priority-oracle".into(),
            w:3,h:3,l:2,wh:9,n:18,ld:vec![1,2],vd:3,nets,pin_owner:pins,
        };
        inst.free_dist=free_space_table(inst.w,inst.h,inst.l,&inst.ld,inst.vd);
        inst
    }

    // Independent dense Dijkstra and tree extraction. No Scratch/search calls.
    fn oracle_route(inst: &Inst, owner: &[i32], nid: usize, reverse: bool) -> Option<Arc<Route>> {
        let net=&inst.nets[nid];
        let mut dist=vec![INF;inst.n as usize];let mut done=vec![false;inst.n as usize];
        dist[net.driver as usize]=0;
        let mut nb=[0;6];let mut wb=[0;6];
        loop {
            let Some(u)=(0..inst.n as usize).filter(|&v|!done[v]&&dist[v]<INF).min_by_key(|&v|(dist[v],v)) else {break;};
            done[u]=true;
            let count=inst.neigh(u as u32,&mut nb,&mut wb);
            for i in 0..count {
                let v=nb[i] as usize;
                if (owner[v]>=0&&owner[v]!=nid as i32)||(inst.pin_owner[v]>=0&&inst.pin_owner[v]!=nid as i32) {continue;}
                dist[v]=dist[v].min(dist[u]+wb[i]);
            }
        }
        let mut edges=std::collections::BTreeSet::new();let mut delay=0;
        for &sink in &net.sinks {
            if dist[sink as usize]==INF {return None;}
            delay+=dist[sink as usize] as u64;
            let mut v=sink;
            while v!=net.driver {
                let count=inst.neigh(v,&mut nb,&mut wb);
                let parent=(0..count).filter_map(|i| {
                    let u=nb[i];
                    (dist[u as usize].checked_add(wb[i])==Some(dist[v as usize])).then_some((dist[u as usize],if reverse {u32::MAX-u}else{u},u))
                }).min()?.2;
                edges.insert(canon(parent,v));v=parent;
            }
        }
        let edges: Vec<_>=edges.into_iter().collect();let mut verts=vec![net.driver];
        verts.extend(edges.iter().flat_map(|&(a,b)|[a,b]));verts.sort_unstable();verts.dedup();
        assert_eq!(edges.len()+1,verts.len());
        assert_eq!(physical_tree_delay(inst,nid,&edges),delay);
        Some(Arc::new(Route{verts,edges,delay}))
    }

    fn oracle_orders(inst: &Inst, owner: &[i32], reverse: bool) -> Vec<Vec<Arc<Route>>> {
        let mut result=Vec::new();
        fn visit(inst:&Inst,owner:&mut [i32],reverse:bool,order:&mut Vec<usize>,routes:&mut Vec<Option<Arc<Route>>>,result:&mut Vec<Vec<Arc<Route>>>) {
            if order.len()==inst.nets.len() {
                result.push(routes.iter().map(|r|Arc::clone(r.as_ref().unwrap())).collect());return;
            }
            for nid in 0..inst.nets.len() {
                if routes[nid].is_some() {continue;}
                if let Some(route)=oracle_route(inst,owner,nid,reverse) {
                    for &v in &route.verts {assert!(owner[v as usize]<0);owner[v as usize]=nid as i32;}
                    order.push(nid);routes[nid]=Some(Arc::clone(&route));
                    visit(inst,owner,reverse,order,routes,result);
                    routes[nid]=None;order.pop();for &v in &route.verts {owner[v as usize]=-1;}
                }
            }
        }
        visit(inst,&mut owner.to_vec(),reverse,&mut Vec::new(),&mut vec![None;inst.nets.len()],&mut result);
        result
    }

    #[test]
    fn priority_closure_matches_independent_floyd_warshall_and_rejects_cycles() {
        let mut rng=Rng::new(92041);
        for n in 2..=16 {
            let mut reach=vec![0;n];let mut edges=vec![vec![false;n];n];
            for _ in 0..100 {
                let high=rng.below(n as u64) as usize;let low=rng.below(n as u64) as usize;
                let mut reference=edges.clone();reference[high][low]=true;
                for k in 0..n {for i in 0..n {for j in 0..n {let through=reference[i][k]&&reference[k][j];reference[i][j]=reference[i][j]||through;}}}
                let got=add_priority(&reach,high,low);
                if (0..n).any(|i|reference[i][i]) {assert!(got.is_none());}
                else {
                    reach=got.unwrap();edges=reference;
                    for i in 0..n {for j in 0..n {assert_eq!(reach[i]&(1u64<<j)!=0,edges[i][j]);}}
                }
            }
        }
    }

    #[test]
    fn priority_results_match_independent_total_order_oracle_with_fixed_obstacles() {
        let mut rng=Rng::new(62173);let mut successes=0;let mut branching=0;
        for trial in 0..96 {
            let mut vertices:Vec<u32>=(0..18).collect();
            for i in (1..vertices.len()).rev() {let j=rng.below((i+1)as u64)as usize;vertices.swap(i,j);}
            let mut pins=vec![-1;18];let mut nets=Vec::new();
            for nid in 0..3 {
                let driver=vertices[3*nid];let mut sinks=vec![vertices[3*nid+1]];
                if trial%2==0 {sinks.push(vertices[3*nid+2]);}
                pins[driver as usize]=nid as i32;for &s in &sinks {pins[s as usize]=nid as i32;}
                nets.push(Net{driver,sinks});
            }
            let inst=fixture(nets,pins);let mut owner=vec![-1;18];owner[vertices[10]as usize]=99;
            let before=owner.clone();
            for reverse in [false,true] {
                let oracle=oracle_orders(&inst,&owner,reverse);
                if oracle.is_empty() {continue;}
                let best=oracle.iter().map(|r|r.iter().map(|r|r.delay).sum::<u64>()).min().unwrap();
                let mut ws=Scratch::new(18,3,491);
                let (got,stats)=search(&inst,&mut owner,&mut ws,&[0,1,2],best,usize::MAX,2,reverse,256);
                assert_eq!(owner,before);
                if let Some(routes)=got {
                    successes+=1;if stats.expanded>1 {branching+=1;}
                    assert!(oracle.iter().any(|r|*r==routes),"trial{trial},reverse{reverse}: result outside priority-order oracle");
                    assert_eq!(routes.iter().map(|r|r.delay).sum::<u64>(),best);
                }
                let (below,_)=search(&inst,&mut owner,&mut ws,&[0,1,2],best-1,usize::MAX,2,reverse,256);
                assert!(below.is_none());assert_eq!(owner,before);
            }
        }
        assert!(successes>16,"oracle never exercises enough successful searches");
        assert!(branching>8,"oracle never exercises enough conflict branches");
    }

    #[test]
    fn priority_group_preserves_fixed_branch_and_rolls_back_on_zero_budget() {
        let mut pins=vec![-1;18];
        for (nid,vs) in [(0,vec![0,2,8]),(1,vec![6,3]),(2,vec![9,12])] {for v in vs {pins[v]=nid;}}
        let inst=fixture(vec![Net{driver:0,sinks:vec![2,8]},Net{driver:6,sinks:vec![3]},Net{driver:9,sinks:vec![12]}],pins);
        for budget in [0,32] {for neutral in 0..=2 {for seed in 0..16 {
            let mut sol=Solution{verts:vec![vec![0,1,2,4,5,8],vec![3,6],vec![9,12]],edges:vec![vec![(0,1),(1,4),(4,5),(2,5),(5,8)],vec![(3,6)],vec![(9,12)]],delay:vec![8,1,2],total:11,vcount:10};
            let old=sol.clone();let mut owner=vec![-1;18];for nid in 0..3 {commit(&mut owner,nid,&sol.verts[nid]);}
            let before=owner.clone();let mut ws=Scratch::new(18,3,seed);
            assert_consistent_state(&inst,&sol,&owner);
            let result=group_move(&inst,&mut sol,&mut owner,&mut ws,&[0,1],neutral,budget);
            assert_consistent_state(&inst,&sol,&owner);assert_eq!(sol.edges[2],old.edges[2]);
            if budget==0 {assert!(result.is_none());assert_eq!(sol.edges,old.edges);assert_eq!(owner,before);assert_eq!(sol.total,11);}
            else {assert!(result.is_some());assert_eq!(sol.total,9);}
        }}}
    }
}
