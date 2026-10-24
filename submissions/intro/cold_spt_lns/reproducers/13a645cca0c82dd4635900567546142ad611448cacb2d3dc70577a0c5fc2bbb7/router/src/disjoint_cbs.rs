//! Private static multi-sink adaptation of Li et al., ICAPS2019 disjoint CBS.
//! Positive membership is represented by an extra zero-demand tree terminal.
//! Bounded search makes no claim of infeasibility or global routing optimality.
use super::*;
use std::sync::atomic::{AtomicU64,Ordering};
static CALLS: AtomicU64=AtomicU64::new(0);
static EXPANDED: AtomicU64=AtomicU64::new(0);
static LOW: AtomicU64=AtomicU64::new(0);
static POSITIVE: AtomicU64=AtomicU64::new(0);
#[derive(Clone)]
struct Route { verts: Vec<u32>, edges: Vec<(u32,u32)>, delay: u64 }
#[derive(Clone)]
struct Node { routes: Vec<Arc<Route>>, bans: Vec<Vec<u32>>, required: Vec<Vec<u32>>, cost: u64 }

fn low_level(inst: &Inst,owner: &mut [i32],ws: &mut Scratch,nid: usize,
             bans: &[u32],required: &[u32],cap: u64) -> Option<Arc<Route>> {
    LOW.fetch_add(1,Ordering::Relaxed);
    if bans.iter().any(|&v| v>=inst.n || inst.pin_owner[v as usize]==nid as i32) {
        return None;
    }
    if required.iter().any(|&v| v>=inst.n || bans.contains(&v) ||
        (inst.pin_owner[v as usize]>=0 && inst.pin_owner[v as usize]!=nid as i32) ||
        (owner[v as usize]>=0 && owner[v as usize]!=nid as i32)) { return None; }
    let mut targets=inst.nets[nid].sinks.clone();
    for &v in required {
        if v!=inst.nets[nid].driver && !targets.contains(&v) { targets.push(v); }
    }
    let saved: Vec<_>=bans.iter().map(|&v| (v,owner[v as usize])).collect();
    for &v in bans { owner[v as usize]=i32::MAX; }
    // A required dangling branch has zero objective demand. Its distance can
    // exceed the entire actual sink objective, so cap is NOT its search cutoff.
    let cutoff=if required.is_empty() { cap.min(u32::MAX as u64) as u32 } else { u32::MAX };
    ws.dijkstra_goals(inst,owner,inst.nets[nid].driver,nid as i32,false,cutoff,&targets);
    let synthetic=Net { driver:inst.nets[nid].driver,sinks:targets };
    let extracted=ws.extract(inst,&synthetic);
    for (v,old) in saved { owner[v as usize]=old; }
    extracted.and_then(|(verts,edges,_)| {
        let delay=physical_tree_delay(inst,nid,&edges);
        if delay>cap { return None; }
        assert!(required.iter().all(|v| verts.binary_search(v).is_ok()));
        assert!(bans.iter().all(|v| verts.binary_search(v).is_err()));
        Some(Arc::new(Route { verts,edges,delay }))
    })
}

fn conflicts(routes: &[Arc<Route>]) -> (usize,Option<(usize,usize,u32)>) {
    let mut total=0;let mut best=None;let mut most=0;
    for a in 0..routes.len() { for b in a+1..routes.len() {
        let (mut i,mut j)=(0,0);let mut common=Vec::new();
        while i<routes[a].verts.len() && j<routes[b].verts.len() {
            let (x,y)=(routes[a].verts[i],routes[b].verts[j]);
            if x==y { common.push(x);i+=1;j+=1; }
            else if x<y { i+=1; } else { j+=1; }
        }
        total+=common.len();
        if common.len()>most { most=common.len();best=Some((a,b,common[common.len()/2])); }
    }}
    (total,best)
}
fn add(values: &mut Vec<u32>,v: u32) {
    if let Err(i)=values.binary_search(&v) { values.insert(i,v); }
}
fn search(inst: &Inst,owner: &mut [i32],ws: &mut Scratch,group: &[usize],
          cap: u64,max_nodes: usize,deadline: Instant) -> Option<Vec<Arc<Route>>> {
    CALLS.fetch_add(1,Ordering::Relaxed);
    let mut routes=Vec::new();let mut cost=0;
    for &nid in group {
        if Instant::now()>=deadline { return None; }
        let r=low_level(inst,owner,ws,nid,&[],&[],cap)?;
        cost+=r.delay;if cost>cap { return None; }routes.push(r);
    }
    let root=Node { routes,bans:vec![Vec::new();group.len()],required:vec![Vec::new();group.len()],cost };
    let root_conflicts=conflicts(&root.routes).0;
    let rank=cost.saturating_add(ws.cbs_queue_weight.saturating_mul(root_conflicts as u64));
    let mut queue=BinaryHeap::from([Reverse((rank,root_conflicts,0usize))]);
    let mut seen=std::collections::HashSet::new();
    seen.insert((root.bans.clone(),root.required.clone()));
    let mut nodes=vec![Some(root)];let mut expanded=0;
    while let Some(Reverse((_,_,id)))=queue.pop() {
        if expanded>=max_nodes || Instant::now()>=deadline { break; }
        expanded+=1;EXPANDED.fetch_add(1,Ordering::Relaxed);
        let node=nodes[id].take().unwrap();
        let Some((a,b,v))=conflicts(&node.routes).1 else { return Some(node.routes); };
        // Choose a deterministic split owner; both membership alternatives are
        // explored. All other group members implicitly exclude a positive v.
        let split=if node.required[a].len()<=node.required[b].len() { a } else { b };
        for positive in [false,true] {
            if Instant::now()>=deadline { break; }
            let mut child=node.clone();let mut affected=Vec::new();
            if positive {
                POSITIVE.fetch_add(1,Ordering::Relaxed);
                add(&mut child.required[split],v);
                // Current split route already contains v and stays minimum-cost.
                for i in 0..group.len() {
                    if i==split { continue; }
                    add(&mut child.bans[i],v);
                    if child.routes[i].verts.binary_search(&v).is_ok() { affected.push(i); }
                }
            } else { add(&mut child.bans[split],v);affected.push(split); }
            if !seen.insert((child.bans.clone(),child.required.clone())) { continue; }
            let mut valid=true;
            for i in affected {
                let others=child.cost-child.routes[i].delay;
                let Some(route)=low_level(inst,owner,ws,group[i],&child.bans[i],&child.required[i],cap-others)
                else { valid=false;break; };
                child.cost=others+route.delay;child.routes[i]=route;
            }
            if !valid { continue; }
            assert!(child.cost>=node.cost);
            let id=nodes.len();let nconflicts=conflicts(&child.routes).0;
            let rank=child.cost.saturating_add(ws.cbs_queue_weight.saturating_mul(nconflicts as u64));
            queue.push(Reverse((rank,nconflicts,id)));
            nodes.push(Some(child));
        }
    }
    None
}

// Remove artificial non-pin leaves only AFTER a complete compatible plan.
fn trim_for_commit(inst: &Inst,nid: usize,route: Arc<Route>) -> Arc<Route> {
    let mut adjacency=std::collections::HashMap::<u32,Vec<u32>>::new();
    for &(a,b) in &route.edges { adjacency.entry(a).or_default().push(b);adjacency.entry(b).or_default().push(a); }
    let mut degree: std::collections::HashMap<_,_>=adjacency.iter().map(|(&v,nb)| (v,nb.len())).collect();
    let mut pending: Vec<_>=route.verts.iter().copied().filter(|v| degree.get(v).copied().unwrap_or(0)==1 && inst.pin_owner[*v as usize]!=nid as i32).collect();
    let mut at=0;
    while at<pending.len() {
        let v=pending[at];at+=1;
        if degree[&v]!=1 {continue;}degree.insert(v,0);
        for &u in &adjacency[&v] {
            if degree[&u]>0 { let next=degree[&u]-1;degree.insert(u,next);
                if next==1 && inst.pin_owner[u as usize]!=nid as i32 {pending.push(u);} }
        }
    }
    let keep=|v: u32| degree.get(&v).copied().unwrap_or(0)>0 || inst.pin_owner[v as usize]==nid as i32;
    let verts=route.verts.iter().copied().filter(|&v| keep(v)).collect();
    let edges: Vec<_>=route.edges.iter().copied().filter(|&(u,v)| keep(u)&&keep(v)).collect();
    assert_eq!(physical_tree_delay(inst,nid,&edges),route.delay);
    Arc::new(Route {verts,edges,delay:route.delay})
}
pub(super) fn group_move(inst: &Inst,sol: &mut Solution,owner: &mut [i32],ws: &mut Scratch,
    group: &[usize],max_nodes: usize,bypass: bool,deadline: Instant) -> Option<(i64,bool)> {
    if bypass { return super::cbs::group_move(inst,sol,owner,ws,group,max_nodes,bypass,deadline); }
    if group.len()<2 { return None; }
    let old: u64=group.iter().map(|&n| sol.delay[n]).sum();if old==0 { return None; }
    for &n in group { uncommit(owner,&sol.verts[n]); }
    let found=search(inst,owner,ws,group,old-1,max_nodes,deadline);
    let Some(routes)=found else { for &n in group { commit(owner,n,&sol.verts[n]); }return None; };
    let routes: Vec<_>=group.iter().zip(routes).map(|(&n,r)| trim_for_commit(inst,n,r)).collect();
    let new: u64=routes.iter().map(|r| r.delay).sum();assert!(new<old);
    let old_v: usize=group.iter().map(|&n| sol.verts[n].len()).sum();
    let new_v: usize=routes.iter().map(|r| r.verts.len()).sum();
    for (&n,r) in group.iter().zip(routes) {
        assert!(r.verts.iter().all(|&v| owner[v as usize]<0));commit(owner,n,&r.verts);
        sol.verts[n]=r.verts.clone();sol.edges[n]=r.edges.clone();sol.delay[n]=r.delay;
    }
    sol.total=sol.total-old+new;sol.vcount=(sol.vcount as usize-old_v+new_v) as u64;
    Some((new as i64-old as i64,true))
}
pub(super) fn print_stats() {
    eprintln!("DISJOINT_STATS calls={} expanded={} low_level={} positive={}",
      CALLS.load(Ordering::Relaxed),EXPANDED.load(Ordering::Relaxed),LOW.load(Ordering::Relaxed),POSITIVE.load(Ordering::Relaxed));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(w: u32,h: u32,l: u32,nets: Vec<Net>,ld: Vec<u32>,vd: u32) -> Inst {
        let n=w*h*l;let mut pins=vec![-1;n as usize];
        for (i,net) in nets.iter().enumerate() {
            pins[net.driver as usize]=i as i32;
            for &s in &net.sinks { pins[s as usize]=i as i32; }
        }
        Inst { early_stop:true,retain_negotiated:false,use_radix:true,use_dial:true,
            use_astar:true,budget_bound:false,free_dist:Vec::new(),check_state:false,
            name:"disjoint-oracle".into(),w,h,l,wh:w*h,n,ld,vd,nets,pin_owner:pins }
    }
    #[test]
    fn disjoint_zero_demand_vertex_does_not_consume_objective_cap() {
        let inst=fixture(3,3,1,vec![Net {driver:0,sinks:vec![1]},Net {driver:6,sinks:vec![7]}],vec![1],2);
        let mut owner=vec![-1;9];owner[4]=9;let original=owner.clone();
        let mut ws=Scratch::new(9,2,31);
        let route=low_level(&inst,&mut owner,&mut ws,0,&[],&[8],1).unwrap();
        assert_eq!(route.delay,1);assert!(route.verts.contains(&8));assert_eq!(owner,original);
        let trimmed=trim_for_commit(&inst,0,route.clone());assert_eq!(trimmed.verts,vec![0,1]);assert_eq!(trimmed.delay,1);
        assert!(low_level(&inst,&mut owner,&mut ws,0,&[0],&[],10).is_none());
        assert!(low_level(&inst,&mut owner,&mut ws,0,&[],&[6],10).is_none());
        assert!(low_level(&inst,&mut owner,&mut ws,0,&[],&[4],10).is_none());
        assert!(low_level(&inst,&mut owner,&mut ws,0,&[8],&[8],10).is_none());
        assert_eq!(owner,original);
    }
    #[test]
    fn disjoint_failed_budget_restores_owner_and_success_updates_state() {
        let inst=fixture(4,3,1,vec![Net {driver:0,sinks:vec![2]},Net {driver:8,sinks:vec![11]}],vec![1],2);
        let mut sol=Solution { verts:vec![vec![0,2,4,5,6],vec![8,9,10,11]],
            edges:vec![vec![(0,4),(4,5),(5,6),(2,6)],vec![(8,9),(9,10),(10,11)]],
            delay:vec![4,3],total:7,vcount:9 };
        let mut owner=vec![-1;12];owner[7]=19;
        for n in 0..2 { commit(&mut owner,n,&sol.verts[n]); }
        let original=owner.clone();let mut ws=Scratch::new(12,2,41);
        assert!(group_move(&inst,&mut sol,&mut owner,&mut ws,&[0,1],0,false,Instant::now()+Duration::from_secs(2)).is_none());
        assert_eq!(owner,original);assert_eq!(sol.total,7);assert_eq!(sol.delay,vec![4,3]);
        assert!(group_move(&inst,&mut sol,&mut owner,&mut ws,&[0,1],100,false,Instant::now()+Duration::from_secs(2)).is_some());
        assert_eq!(sol.total,5);assert_eq!(owner[7],19);
        for n in 0..2 {
            assert_eq!(physical_tree_delay(&inst,n,&sol.edges[n]),sol.delay[n]);
            for &v in &sol.verts[n] { assert_eq!(owner[v as usize],n as i32); }
        }
    }
    fn enumerate(inst: &Inst, nid: usize) -> Vec<Route> {
        let mut all = Vec::new();
        let mut nb = [0; 6];
        let mut wb = [0; 6];
        for v in 0..inst.n {
            let k = inst.neigh(v, &mut nb, &mut wb);
            for &u in &nb[..k] {
                if v < u {
                    all.push((v, u));
                }
            }
        }
        let mut out = Vec::new();
        for mask in 1u64..1u64 << all.len() {
            let edges: Vec<_> = all
                .iter()
                .enumerate()
                .filter_map(|(i, &e)| if mask >> i & 1 == 1 { Some(e) } else { None })
                .collect();
            let mut verts: Vec<_> = edges.iter().flat_map(|&(a, b)| [a, b]).collect();
            verts.sort_unstable();
            verts.dedup();
            if edges.len() + 1 != verts.len()
                || verts.iter().any(|&v| {
                    inst.pin_owner[v as usize] >= 0 && inst.pin_owner[v as usize] != nid as i32
                })
            {
                continue;
            }
            let net = &inst.nets[nid];
            if !verts.contains(&net.driver) || net.sinks.iter().any(|s| !verts.contains(s)) {
                continue;
            }
            let mut d = vec![u64::MAX; inst.n as usize];
            d[net.driver as usize] = 0;
            for _ in 0..inst.n {
                for &(a, b) in &edges {
                    let w = edge_w(inst, a, b).unwrap() as u64;
                    d[b as usize] = d[b as usize].min(d[a as usize].saturating_add(w));
                    d[a as usize] = d[a as usize].min(d[b as usize].saturating_add(w));
                }
            }
            if verts.iter().any(|&v| d[v as usize] == u64::MAX) {
                continue;
            }
            let delay = net.sinks.iter().map(|&s| d[s as usize]).sum();
            out.push(Route {
                verts,
                edges,
                delay,
            });
        }
        out
    }

    #[test]
    fn disjoint_matches_exhaustive_trees_and_zero_demand_constraints() {
        let mut feasible=0;let mut infeasible=0;
        for seed in 1..=32u64 {
            let layered=seed%2==0;
            let (w,h,l)=if layered { (2,2,2) } else { (3,3,1) };
            let n=w*h*l;let mut p: Vec<u32>=(0..n).collect();let mut rng=Rng::new(seed*71);
            for i in (1..p.len()).rev() { let j=rng.below((i+1) as u64) as usize;p.swap(i,j); }
            let nets=vec![Net {driver:p[0],sinks:if seed%3==0 {vec![p[1],p[4]]} else {vec![p[1]]}},Net {driver:p[2],sinks:vec![p[3]]}];
            let inst=fixture(w,h,l,nets,(0..l).map(|z| 1+(seed as u32+z)%3).collect(),2);
            let mut owner=vec![-1;n as usize];if seed%4==0 { owner[p[7] as usize]=99; }
            let original=owner.clone();
            let pools: Vec<Vec<_>>=(0..2).map(|nid| enumerate(&inst,nid).into_iter().filter(|r| r.verts.iter().all(|&v| owner[v as usize]<0)).collect()).collect();
            let best=pools[0].iter().flat_map(|a| pools[1].iter().filter_map(move |b|
                if a.verts.iter().all(|v| !b.verts.contains(v)) {Some(a.delay+b.delay)} else {None})).min();
            if best.is_some() {feasible+=1;} else {infeasible+=1;}
            let mut ws=Scratch::new(n,2,seed);
            let found=search(&inst,&mut owner,&mut ws,&[0,1],best.unwrap_or(1000),100_000,Instant::now()+Duration::from_secs(10));
            assert_eq!(found.as_ref().map(|rs| rs.iter().map(|r| r.delay).sum::<u64>()),best,"seed={seed}");
            assert_eq!(owner,original);
            if let Some(best)=best {
                assert!(search(&inst,&mut owner,&mut ws,&[0,1],best-1,100_000,Instant::now()+Duration::from_secs(10)).is_none());
            }
            // Independently enumerate every legal tree containing an extra
            // non-pin terminal, with an optional forbidden vertex.
            let required=[p[5]];let bans=if seed%3==0 {vec![p[6]]} else {Vec::new()};
            let constrained=pools[0].iter().filter(|r| required.iter().all(|v| r.verts.contains(v)) && bans.iter().all(|v| !r.verts.contains(v))).map(|r| r.delay).min();
            let route=low_level(&inst,&mut owner,&mut ws,0,&bans,&required,1000);
            assert_eq!(route.map(|r| r.delay),constrained,"positive oracle seed={seed}");
            assert_eq!(owner,original);
        }
        assert!(feasible>0 && infeasible>0);
    }

    #[test]
    fn disjoint_groups_two_through_eight_match_independent_path_oracle() {
        fn paths(inst: &Inst, owner: &[i32], nid: usize) -> Vec<(u32, u64)> {
            fn visit(
                inst: &Inst,
                owner: &[i32],
                nid: usize,
                v: u32,
                mask: u32,
                cost: u64,
                out: &mut Vec<(u32, u64)>,
            ) {
                if v == inst.nets[nid].sinks[0] {
                    out.push((mask, cost));
                    return;
                }
                let (x, y) = (v % 4, v / 4);
                for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if !(0..4).contains(&nx) || !(0..4).contains(&ny) {
                        continue;
                    }
                    let u = (ny * 4 + nx) as u32;
                    if mask & (1 << u) != 0
                        || owner[u as usize] >= 0
                        || (inst.pin_owner[u as usize] >= 0
                            && inst.pin_owner[u as usize] != nid as i32)
                    {
                        continue;
                    }
                    visit(
                        inst,
                        owner,
                        nid,
                        u,
                        mask | (1 << u),
                        cost + inst.ld[0] as u64,
                        out,
                    );
                }
            }
            let root = inst.nets[nid].driver;
            let mut out = Vec::new();
            visit(inst, owner, nid, root, 1 << root, 0, &mut out);
            out.sort_unstable();
            out.dedup();
            out.sort_by_key(|&(mask, cost)| (cost, mask));
            out
        }
        fn choose(pool: &[Vec<(u32, u64)>], at: usize, used: u32, cost: u64, best: &mut u64) {
            if cost >= *best {
                return;
            }
            if at == pool.len() {
                *best = cost;
                return;
            }
            for &(mask, extra) in &pool[at] {
                if used & mask == 0 {
                    choose(pool, at + 1, used | mask, cost + extra, best);
                }
            }
        }

        let mut feasible=0;let mut infeasible=0;
        for count in 2..=8 { for seed in 0..=8u64 {
            let mut p: Vec<u32>=(0..16).collect();
            if seed>0 { let mut rng=Rng::new(seed*31+count as u64);
                for i in (1..p.len()).rev() { let j=rng.below((i+1) as u64) as usize;p.swap(i,j); }
            }
            let nets=(0..count).map(|i| Net {driver:p[2*i],sinks:vec![p[2*i+1]]}).collect();
            let inst=fixture(4,4,1,nets,vec![1+seed as u32%3],2);
            let mut owner=vec![-1;16];
            if seed%2==1 { if let Some(v)=(0..16).find(|&v| inst.pin_owner[v]<0) { owner[v]=99; } }
            let original=owner.clone();let pools: Vec<_>=(0..count).map(|n| paths(&inst,&owner,n)).collect();
            let mut optimum=u64::MAX;choose(&pools,0,0,0,&mut optimum);
            let expected=if optimum==u64::MAX {infeasible+=1;None} else {feasible+=1;Some(optimum)};
            let group: Vec<_>=(0..count).collect();let mut ws=Scratch::new(16,count,seed);
            let found=search(&inst,&mut owner,&mut ws,&group,expected.unwrap_or(1000),100_000,Instant::now()+Duration::from_secs(20));
            assert_eq!(found.as_ref().map(|r| r.iter().map(|r|r.delay).sum::<u64>()),expected,"count={count} seed={seed}");
            if let Some(routes)=found {
                let mut used=0u32;
                for route in routes { let mask=route.verts.iter().fold(0,|mask,&v| mask|(1<<v));assert_eq!(used&mask,0);used|=mask; }
                assert!(search(&inst,&mut owner,&mut ws,&group,optimum-1,100_000,Instant::now()+Duration::from_secs(20)).is_none());
            }
            assert_eq!(owner,original);
        }}
        assert!(feasible>0 && infeasible>0);
    }

    #[test]
    fn weighted_queues_preserve_feasibility_at_independent_tree_optimum() {
        for seed in 1..=20u64 {
            let mut p: Vec<u32>=(0..8).collect();let mut rng=Rng::new(seed*113);
            for i in (1..p.len()).rev() { let j=rng.below((i+1) as u64) as usize;p.swap(i,j); }
            let inst=fixture(2,2,2,vec![Net {driver:p[0],sinks:vec![p[1]]},Net {driver:p[2],sinks:vec![p[3]]}],vec![1,3],2);
            let a=enumerate(&inst,0);let b=enumerate(&inst,1);
            let optimum=a.iter().flat_map(|a| b.iter().filter_map(move |b|
                if a.verts.iter().all(|v| !b.verts.contains(v)) {Some(a.delay+b.delay)} else {None})).min();
            for weight in [0,1,4,16,64] {
                let mut owner=vec![-1;8];let mut ws=Scratch::new(8,2,seed);ws.cbs_queue_weight=weight;
                let cap=optimum.unwrap_or(1000);let deadline=Instant::now()+Duration::from_secs(10);
                let found=search(&inst,&mut owner,&mut ws,&[0,1],cap,100_000,deadline);
                assert_eq!(found.map(|r| r.iter().map(|r|r.delay).sum::<u64>()),optimum,"disjoint seed={seed} weight={weight}");
                assert_eq!(super::super::cbs::test_weighted_cost(&inst,&mut owner,&mut ws,&[0,1],cap),optimum,"legacy seed={seed} weight={weight}");
                assert!(owner.iter().all(|&v| v<0));
            }
        }
    }

}
